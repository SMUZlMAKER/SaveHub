use std::io;
use std::path::Path;

fn create_directory_symlink<P: AsRef<Path>, Q: AsRef<Path>>(source: P, target: Q) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        junction::create(source, target)
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::os::unix::fs::symlink(source, target)
    }
}

fn main() -> io::Result<()> {
    create_directory_symlink("src", "src.link")?;
    Ok(())
}
