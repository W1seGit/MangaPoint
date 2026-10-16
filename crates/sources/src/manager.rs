//! First-run choices, installed sources and custom extension stores.

use crate::defaults::{self, DefaultSource, KEIYOUSHI_INDEX};
use catalog::{Catalog, CatalogError, Extension, Source};
use library::{Choice, SourceState};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourcesError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error("'{0}' is not an https address")]
    InvalidUrl(String),
    #[error("{0} is not offered by the extension store")]
    NotAvailable(String),
    #[error("'{0}' is not one of the built-in sources")]
    UnknownDefault(String),
}

/// One built-in source as shown on the first-run screen.
#[derive(Debug, Clone)]
pub struct Offer {
    pub default: &'static DefaultSource,
    /// The extension store actually has it right now.
    pub available: bool,
    pub installed: bool,
    /// The user's earlier answer, if any.
    pub choice: Option<Choice>,
}

/// An installed source with what we know about it.
#[derive(Debug, Clone)]
pub struct InstalledSource {
    pub source: Source,
    pub extension: Extension,
    /// Set when it is one of the built-in sources.
    pub default: Option<&'static DefaultSource>,
}

pub struct SourceManager<'a, C: Catalog> {
    catalog: &'a C,
}

impl<'a, C: Catalog> SourceManager<'a, C> {
    pub fn new(catalog: &'a C) -> Self {
        Self { catalog }
    }

    /// Add the Keiyoushi store the first time. Safe to call on every start.
    pub async fn ensure_default_store(&self, state: &mut SourceState) -> Result<(), SourcesError> {
        if !state.default_store_added {
            self.catalog.add_extension_store(KEIYOUSHI_INDEX).await?;
            state.default_store_added = true;
        }
        Ok(())
    }

    /// The built-in list with each source's availability and the user's answer.
    pub async fn offers(&self, state: &SourceState) -> Result<Vec<Offer>, SourcesError> {
        let extensions = self.catalog.fetch_extensions().await?;
        Ok(defaults::DEFAULTS
            .iter()
            .map(|d| {
                let ext = extensions.iter().find(|e| e.pkg_name == d.pkg_name());
                Offer {
                    default: d,
                    available: ext.is_some(),
                    installed: ext.is_some_and(|e| e.is_installed),
                    choice: state.choices.get(d.id).copied(),
                }
            })
            .collect())
    }

    /// Record the user's answer; `Install` also installs the extension.
    pub async fn answer(
        &self,
        state: &mut SourceState,
        id: &str,
        choice: Choice,
    ) -> Result<(), SourcesError> {
        let default = defaults::find(id).ok_or_else(|| SourcesError::UnknownDefault(id.into()))?;
        if choice == Choice::Install {
            self.install_pkg(&default.pkg_name()).await?;
        }
        state.choices.insert(id.to_string(), choice);
        Ok(())
    }

    /// Mark first-run setup as finished. Unanswered sources count as skipped.
    pub fn finish_onboarding(&self, state: &mut SourceState) {
        for d in defaults::DEFAULTS {
            state
                .choices
                .entry(d.id.to_string())
                .or_insert(Choice::Skip);
        }
        state.onboarding_done = true;
    }

    /// Add a third-party extension store and return what it offers.
    pub async fn add_store(
        &self,
        state: &mut SourceState,
        url: &str,
    ) -> Result<Vec<Extension>, SourcesError> {
        let url = validate_store_url(url)?;
        self.catalog.add_extension_store(&url).await?;
        if !state.custom_stores.contains(&url) {
            state.custom_stores.push(url);
        }
        Ok(self.catalog.fetch_extensions().await?)
    }

    /// Install an extension picked by hand from a store.
    pub async fn install_custom(
        &self,
        state: &mut SourceState,
        pkg_name: &str,
    ) -> Result<(), SourcesError> {
        self.install_pkg(pkg_name).await?;
        if defaults::find_by_pkg(pkg_name).is_none() {
            state.custom_extensions.insert(pkg_name.to_string());
        }
        Ok(())
    }

    pub async fn remove(
        &self,
        state: &mut SourceState,
        pkg_name: &str,
    ) -> Result<(), SourcesError> {
        self.catalog.uninstall_extension(pkg_name).await?;
        state.custom_extensions.remove(pkg_name);
        if let Some(d) = defaults::find_by_pkg(pkg_name) {
            state.choices.insert(d.id.to_string(), Choice::Skip);
        }
        Ok(())
    }

    /// Installed sources in `language` (or language-independent ones).
    pub async fn installed(&self, language: &str) -> Result<Vec<InstalledSource>, SourcesError> {
        let extensions = self.catalog.fetch_extensions().await?;
        let sources = self.catalog.list_sources().await?;
        let mut out = Vec::new();
        for ext in extensions.iter().filter(|e| e.is_installed) {
            if let Some(source) = source_for_extension(ext, &sources, language) {
                out.push(InstalledSource {
                    source: source.clone(),
                    extension: ext.clone(),
                    default: defaults::find_by_pkg(&ext.pkg_name),
                });
            }
        }
        Ok(out)
    }

    async fn install_pkg(&self, pkg_name: &str) -> Result<(), SourcesError> {
        let extensions = self.catalog.fetch_extensions().await?;
        let ext = extensions
            .iter()
            .find(|e| e.pkg_name == pkg_name)
            .ok_or_else(|| SourcesError::NotAvailable(pkg_name.to_string()))?;
        if !ext.is_installed {
            self.catalog.install_extension(pkg_name).await?;
        }
        Ok(())
    }
}

/// Match an extension to the source it provides: one from that extension
/// (or, when the server does not say, of the same name), in the extension's
/// own language, or `language` for multi-language ("all") ones.
pub fn source_for_extension<'s>(
    ext: &Extension,
    sources: &'s [Source],
    language: &str,
) -> Option<&'s Source> {
    let want = if ext.lang == "all" {
        language
    } else {
        &ext.lang
    };
    let from_ext = |s: &&Source| match &s.extension_pkg {
        Some(pkg) => *pkg == ext.pkg_name,
        None => s.name == ext.name,
    };
    sources
        .iter()
        .filter(from_ext)
        .find(|s| s.lang == want)
        .or_else(|| sources.iter().filter(from_ext).find(|s| s.lang == "all"))
}

/// Only plain https addresses are accepted as extension stores.
pub fn validate_store_url(url: &str) -> Result<String, SourcesError> {
    let url = url.trim();
    let bad = || SourcesError::InvalidUrl(url.to_string());
    let rest = url.strip_prefix("https://").ok_or_else(bad)?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let ok = !host.is_empty()
        && host.contains('.')
        && !host.contains('@')
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    if ok {
        Ok(url.to_string())
    } else {
        Err(bad())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::fake::FakeCatalog;

    fn catalog_with_two_defaults() -> FakeCatalog {
        let c = FakeCatalog::new();
        c.add_extension(
            "eu.kanade.tachiyomi.extension.en.mangapill",
            "MangaPill",
            "en",
            "s-pill",
        );
        c.add_extension(
            "eu.kanade.tachiyomi.extension.all.mangadex",
            "MangaDex",
            "all",
            "s-dex-en",
        );
        c.add_extension("com.example.custom", "Custom", "en", "s-custom");
        c
    }

    #[tokio::test]
    async fn default_store_is_added_exactly_once() {
        let c = FakeCatalog::new();
        let m = SourceManager::new(&c);
        let mut st = SourceState::default();
        m.ensure_default_store(&mut st).await.unwrap();
        m.ensure_default_store(&mut st).await.unwrap();
        assert_eq!(c.stores(), vec![KEIYOUSHI_INDEX.to_string()]);
        assert!(st.default_store_added);
    }

    #[tokio::test]
    async fn offers_show_availability_and_answers() {
        let c = catalog_with_two_defaults();
        let m = SourceManager::new(&c);
        let mut st = SourceState::default();
        m.answer(&mut st, "en.mangapill", Choice::Install)
            .await
            .unwrap();
        m.answer(&mut st, "all.mangadex", Choice::Skip)
            .await
            .unwrap();

        let offers = m.offers(&st).await.unwrap();
        assert_eq!(offers.len(), defaults::DEFAULTS.len());
        let pill = offers
            .iter()
            .find(|o| o.default.id == "en.mangapill")
            .unwrap();
        assert!(pill.available && pill.installed);
        assert_eq!(pill.choice, Some(Choice::Install));
        let dex = offers
            .iter()
            .find(|o| o.default.id == "all.mangadex")
            .unwrap();
        assert!(dex.available && !dex.installed);
        assert_eq!(dex.choice, Some(Choice::Skip));
        // Not in this fake store at all:
        let fire = offers
            .iter()
            .find(|o| o.default.id == "all.mangafire")
            .unwrap();
        assert!(!fire.available);
        assert_eq!(fire.choice, None);
    }

    #[tokio::test]
    async fn skipping_does_not_install_and_unavailable_sources_cannot_be_installed() {
        let c = catalog_with_two_defaults();
        let m = SourceManager::new(&c);
        let mut st = SourceState::default();
        m.answer(&mut st, "all.mangadex", Choice::Skip)
            .await
            .unwrap();
        assert!(!c.is_installed("eu.kanade.tachiyomi.extension.all.mangadex"));
        assert!(c.calls().iter().all(|call| !call.starts_with("install")));

        let err = m
            .answer(&mut st, "all.mangafire", Choice::Install)
            .await
            .unwrap_err();
        assert!(matches!(err, SourcesError::NotAvailable(_)));
        assert!(
            !st.choices.contains_key("all.mangafire"),
            "a failed install is not recorded as an answer"
        );

        let err = m
            .answer(&mut st, "nope", Choice::Install)
            .await
            .unwrap_err();
        assert!(matches!(err, SourcesError::UnknownDefault(_)));
    }

    #[tokio::test]
    async fn already_installed_extensions_are_not_installed_again() {
        let c = catalog_with_two_defaults();
        let m = SourceManager::new(&c);
        let mut st = SourceState::default();
        m.answer(&mut st, "en.mangapill", Choice::Install)
            .await
            .unwrap();
        m.answer(&mut st, "en.mangapill", Choice::Install)
            .await
            .unwrap();
        let installs = c
            .calls()
            .iter()
            .filter(|c| c.starts_with("install"))
            .count();
        assert_eq!(installs, 1);
    }

    #[tokio::test]
    async fn finishing_onboarding_marks_the_rest_as_skipped() {
        let c = catalog_with_two_defaults();
        let m = SourceManager::new(&c);
        let mut st = SourceState::default();
        m.answer(&mut st, "en.mangapill", Choice::Install)
            .await
            .unwrap();
        m.finish_onboarding(&mut st);
        assert!(st.onboarding_done);
        assert_eq!(st.choices["en.mangapill"], Choice::Install);
        assert_eq!(st.choices["all.mangafire"], Choice::Skip);
        assert!(st
            .unanswered(defaults::DEFAULTS.iter().map(|d| d.id))
            .is_empty());
    }

    #[tokio::test]
    async fn custom_stores_are_validated_remembered_and_listed() {
        let c = catalog_with_two_defaults();
        let m = SourceManager::new(&c);
        let mut st = SourceState::default();

        assert!(matches!(
            m.add_store(&mut st, "http://insecure.example/index.json")
                .await,
            Err(SourcesError::InvalidUrl(_))
        ));
        assert!(
            c.stores().is_empty(),
            "nothing is sent for a rejected address"
        );

        let exts = m
            .add_store(&mut st, " https://example.org/repo/index.pb ")
            .await
            .unwrap();
        assert_eq!(exts.len(), 3);
        assert_eq!(st.custom_stores, vec!["https://example.org/repo/index.pb"]);
        m.add_store(&mut st, "https://example.org/repo/index.pb")
            .await
            .unwrap();
        assert_eq!(st.custom_stores.len(), 1, "no duplicates");

        m.install_custom(&mut st, "com.example.custom")
            .await
            .unwrap();
        assert!(st.custom_extensions.contains("com.example.custom"));
        // Built-ins installed through the custom path are not double-booked.
        m.install_custom(&mut st, "eu.kanade.tachiyomi.extension.en.mangapill")
            .await
            .unwrap();
        assert_eq!(st.custom_extensions.len(), 1);
    }

    #[tokio::test]
    async fn installed_sources_are_matched_to_their_extension_and_default() {
        let c = catalog_with_two_defaults();
        let m = SourceManager::new(&c);
        let mut st = SourceState::default();
        m.answer(&mut st, "en.mangapill", Choice::Install)
            .await
            .unwrap();
        m.install_custom(&mut st, "com.example.custom")
            .await
            .unwrap();
        let installed = m.installed("en").await.unwrap();
        let ids: Vec<_> = installed.iter().map(|i| i.source.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"s-pill") && ids.contains(&"s-custom"));
        let pill = installed.iter().find(|i| i.source.id == "s-pill").unwrap();
        assert_eq!(pill.default.unwrap().id, "en.mangapill");
        assert!(installed
            .iter()
            .find(|i| i.source.id == "s-custom")
            .unwrap()
            .default
            .is_none());
    }

    #[test]
    fn a_source_named_unlike_its_extension_is_found_by_package() {
        let ext = Extension {
            pkg_name: "eu.kanade.tachiyomi.extension.uk.mangainua".into(),
            name: "MangaInUa".into(),
            lang: "uk".into(),
            is_installed: true,
            is_nsfw: false,
            has_update: false,
        };
        let source = |id: &str, name: &str, pkg: Option<&str>| Source {
            id: id.into(),
            name: name.into(),
            lang: "uk".into(),
            is_nsfw: false,
            extension_pkg: pkg.map(String::from),
        };
        let sources = [
            source("other", "MangaInUa", Some("eu.kanade.tachiyomi.extension.uk.other")),
            source("inua", "MANGA/in/UA", Some("eu.kanade.tachiyomi.extension.uk.mangainua")),
        ];
        assert_eq!(source_for_extension(&ext, &sources, "en").unwrap().id, "inua");
        // A server that does not name the package: matched by name, as before.
        let unnamed = [source("plain", "MangaInUa", None)];
        assert_eq!(source_for_extension(&ext, &unnamed, "en").unwrap().id, "plain");
    }

    #[tokio::test]
    async fn removing_a_default_records_it_as_skipped() {
        let c = catalog_with_two_defaults();
        let m = SourceManager::new(&c);
        let mut st = SourceState::default();
        m.answer(&mut st, "en.mangapill", Choice::Install)
            .await
            .unwrap();
        m.remove(&mut st, "eu.kanade.tachiyomi.extension.en.mangapill")
            .await
            .unwrap();
        assert!(!c.is_installed("eu.kanade.tachiyomi.extension.en.mangapill"));
        assert_eq!(st.choices["en.mangapill"], Choice::Skip);
    }

    #[test]
    fn store_urls_must_be_https_with_a_real_host() {
        for ok in [
            "https://example.org/index.pb",
            "https://raw.githubusercontent.com/a/b/repo/index.json",
        ] {
            assert!(validate_store_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://example.org/x",
            "ftp://example.org/x",
            "javascript:alert(1)",
            "https://",
            "https://localhost/x",
            "https://user@evil.example/x",
            "https://exa mple.org/x",
            "",
        ] {
            assert!(validate_store_url(bad).is_err(), "{bad:?}");
        }
    }
}
