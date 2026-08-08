//! Operating-system application directories shared by configuration and persistence.

use std::{path::PathBuf, sync::LazyLock};

use directories::ProjectDirs;

static PROJECT_DIRS: LazyLock<ProjectDirs> = LazyLock::new(|| {
    ProjectDirs::from("", "", "fjarsyn")
        .expect("unable to get required project directories from OS")
});

pub(crate) static CONFIG_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| PROJECT_DIRS.config_dir().to_path_buf());

pub(crate) static DATA_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| PROJECT_DIRS.data_dir().to_path_buf());
