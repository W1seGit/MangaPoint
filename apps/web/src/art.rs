//! The desktop app's self-contained illustrations, included from the app's
//! own files so there is one source of truth for the cat, the ocean and the
//! shelf. They only need dioxus. (They find each other as `super::name`, which
//! is why they are declared side by side here.)

#[path = "../../desktop/src/ui/cat.rs"]
pub mod cat;
#[path = "../../desktop/src/ui/hang.rs"]
pub mod hang;
#[path = "../../desktop/src/ui/icons.rs"]
pub mod icons;
#[path = "../../desktop/src/ui/ocean.rs"]
pub mod ocean;
#[path = "../../desktop/src/ui/pixels.rs"]
pub mod pixels;
#[path = "../../desktop/src/ui/placeholder.rs"]
pub mod placeholder;
#[path = "../../desktop/src/ui/scenes.rs"]
pub mod scenes;
#[path = "../../desktop/src/ui/shelf.rs"]
pub mod shelf;
#[path = "../../desktop/src/ui/walker.rs"]
pub mod walker;
#[path = "desk_art.rs"]
pub mod desk;
