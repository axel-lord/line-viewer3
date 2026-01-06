use std::path::{Path, PathBuf};

pub trait PathExt {
    fn canonicalize_at(&self, dest: &Self) -> std::io::Result<PathBuf>;
}

impl PathExt for Path {
    fn canonicalize_at(&self, dest: &Self) -> std::io::Result<PathBuf> {
        fn internal(dest: &Path, path: &Path) -> std::io::Result<PathBuf> {
            std::env::set_current_dir(dest)?;
            path.canonicalize()
        }

        let s = std::env::current_dir().expect("should be able to get current directory");

        let s = if s.is_absolute() {
            s
        } else {
            s.canonicalize()
                .expect("should be able to canonicalize current directory")
        };

        let r = internal(dest, self);
        std::env::set_current_dir(s)
            .expect("should be able to restore current directory to a prior value");
        r
    }
}
