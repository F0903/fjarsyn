pub use fjarsyn_core::app::Route;

#[derive(Debug, Clone)]
pub enum NavigationMessage {
    Navigate(Route),
    NavigateWithBack(Route),
    Back,
}
