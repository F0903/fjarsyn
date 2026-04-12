pub use fjarsyn_core::navigation::Route;

#[derive(Debug, Clone)]
pub enum NavigationMessage {
    Navigate(Route),
    NavigateWithBack(Route),
    Back,
}
