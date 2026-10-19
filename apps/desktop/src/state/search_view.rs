//! The search page's results.

use search::RankedSeries;

#[derive(Clone, Default)]
pub struct SearchView {
    pub query: String,
    pub ranked: Vec<RankedSeries>,
    /// Sources are still being asked.
    pub searching: bool,
    /// Every source has answered; the chapter lists of the best-looking series
    /// are being looked at, to pick the best source for each. The results are
    /// all there and can be used meanwhile.
    pub enriching: bool,
    pub answered: usize,
    pub total: usize,
    /// Sources that did not answer, by name.
    pub failed: Vec<String>,
    pub no_sources: bool,
    /// Every installed source is turned off for searching.
    pub none_selected: bool,
    /// Which search these results belong to. A newer search bumps it, and an
    /// older one still running then stops publishing.
    pub generation: u64,
}
