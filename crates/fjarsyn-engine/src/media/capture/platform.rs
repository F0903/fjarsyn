use super::{Error, PlatformProvider, Provider as ProviderContract};

pub type PlatformItem = <PlatformProvider as ProviderContract>::Item;

pub fn pick_platform_item(
    raw_window_handle: u64,
) -> Result<impl std::future::Future<Output = Result<Option<PlatformItem>, Error>> + Send, Error> {
    let picker = super::windows::selection::user_pick_capture_item(raw_window_handle)
        .map_err(Error::from)?;
    Ok(async move { picker.await.map_err(Error::from) })
}
