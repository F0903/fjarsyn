//! mDNS observation source and admission-bounded discovery aggregation.

mod backend;
mod endpoint_hints;
mod mdns_backend;
mod registry;

pub(super) use backend::{Backend, Observation, ResolvedAdvertisement};
use endpoint_hints::normalize_endpoint_hints;
pub(super) use mdns_backend::MdnsBackend;
pub(super) use registry::Registry;
