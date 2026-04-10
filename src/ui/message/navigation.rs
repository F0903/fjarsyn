#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    Home,
    Messages { peer_id: Option<String> },
    Contacts,
    Call,
    Settings,
}

impl Route {
    pub fn same_screen(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

#[derive(Debug, Clone)]
pub enum NavigationMessage {
    Navigate(Route),
    NavigateWithBack(Route),
    Back,
}
