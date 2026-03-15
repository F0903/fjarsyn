#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    Home,
    Contacts,
    Call,
    Settings,
}

#[derive(Debug, Clone)]
pub enum NavigationMessage {
    Navigate(Route),
    NavigateWithBack(Route),
    Back,
}
