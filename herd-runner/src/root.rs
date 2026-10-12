//! The one data root, and the per-job workspace inside it (E-45).
//!
//! A runner is given exactly one directory it may write in. It is validated
//! at startup — a root it cannot use is a refusal naming the path, never a
//! runner that joins the hub and fails on its first job — and every path the
//! runner derives afterwards is resolved through [`DataRoot`], which refuses
//! anything that would leave it.
//!
//! What this does and does not promise. Everything *Herd* writes on a job's
//! behalf is inside the root: the job's working directory, its `HOME`, its
//! `TMPDIR`, and from M11 the content store. A job's own environment is built
//! from scratch rather than inherited, so no variable from the runner's shell
//! can point its writes elsewhere. What M4 does **not** do is stop a job that
//! names an absolute path outside the root: that needs a mount namespace, and
//! until one exists the promise is "Herd writes nowhere else, and a job is
//! given nowhere else to write", which is the promise the tests assert.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use herd_proto::JobId;

/// Where a runner keeps everything, unless its owner says otherwise.
const DEFAULT_DIR: &str = "herd-runner";

/// The directory holding one subdirectory per job.
const JOBS: &str = "jobs";

/// One validated data root. Holding this value is the proof that the
/// directory exists, is a directory, and is writable by this runner.
#[derive(Debug, Clone)]
pub struct DataRoot {
    path: PathBuf,
}

impl DataRoot {
    /// Where the data root lives when the owner names none: the XDG state
    /// directory, which is per-user and never a system path. With neither
    /// `XDG_STATE_HOME` nor `HOME` set there is no sensible guess, and a
    /// guess is the one thing a data root must not be — so it is an error
    /// saying which variable to set.
    pub fn default_path() -> Result<PathBuf, String> {
        if let Some(state) = non_empty_var("XDG_STATE_HOME") {
            return Ok(PathBuf::from(state).join(DEFAULT_DIR));
        }
        if let Some(home) = non_empty_var("HOME") {
            return Ok(PathBuf::from(home)
                .join(".local")
                .join("state")
                .join(DEFAULT_DIR));
        }
        Err(
            "no data root was given and neither HERD_DATA_ROOT, XDG_STATE_HOME \
             nor HOME is set, so there is nowhere to put one"
                .to_owned(),
        )
    }

    /// Validate a data root, creating it if it does not exist yet.
    ///
    /// Every failure names the path, because the only useful thing to say
    /// about a root a runner cannot use is which one it was.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let given = path.as_ref();
        if given.as_os_str().is_empty() {
            return Err("the data root is an empty path".to_owned());
        }
        fs::create_dir_all(given).map_err(|error| {
            format!(
                "the data root {} cannot be created: {error}",
                given.display()
            )
        })?;
        // Canonical from here on: a root reached through a symlink would make
        // "inside the root" a question about the link rather than the disk.
        let path = fs::canonicalize(given).map_err(|error| {
            format!(
                "the data root {} cannot be resolved: {error}",
                given.display()
            )
        })?;
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("the data root {} cannot be read: {error}", path.display()))?;
        if !metadata.is_dir() {
            return Err(format!(
                "the data root {} is not a directory",
                path.display()
            ));
        }
        if path.parent().is_none() {
            return Err(format!(
                "the data root {} is the filesystem root, which a runner may not own",
                path.display()
            ));
        }
        // Writability is tested by writing, not by reading permission bits:
        // a read-only mount, a full disk and a wrong owner all look the same
        // from the bits and all have to be refused here rather than later.
        let probe = path.join(".herd-writable");
        fs::write(&probe, b"herd").map_err(|error| {
            format!("the data root {} is not writable: {error}", path.display())
        })?;
        let _ = fs::remove_file(&probe);
        fs::create_dir_all(path.join(JOBS)).map_err(|error| {
            format!(
                "the data root {} cannot hold a jobs directory: {error}",
                path.display()
            )
        })?;
        Ok(Self { path })
    }

    /// The validated root itself.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Resolve a path that must stay inside the root, refusing anything that
    /// would leave it and naming the path it refused.
    ///
    /// This is the single gate every derived path goes through, so that
    /// "nothing outside the root" is one function with one test rather than a
    /// habit repeated at each call site.
    pub fn resolve(&self, relative: impl AsRef<Path>) -> Result<PathBuf, String> {
        let relative = relative.as_ref();
        let mut resolved = self.path.clone();
        for component in relative.components() {
            match component {
                Component::Normal(part) => resolved.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(format!(
                        "{} is outside the data root {}",
                        relative.display(),
                        self.path.display()
                    ))
                }
            }
        }
        if !resolved.starts_with(&self.path) {
            return Err(format!(
                "{} is outside the data root {}",
                relative.display(),
                self.path.display()
            ));
        }
        Ok(resolved)
    }

    /// Make this job's workspace: its working directory, its `HOME` and its
    /// `TMPDIR`, all inside the root, all empty.
    pub fn open_workspace(&self, job_id: &JobId) -> Result<Workspace, String> {
        let name = directory_name(job_id)?;
        let base = self.resolve(Path::new(JOBS).join(&name))?;
        // A leftover from a previous life of the same id would make the disk
        // budget and the measurements lie, so the workspace starts empty.
        if base.exists() {
            fs::remove_dir_all(&base).map_err(|error| {
                format!(
                    "the workspace {} is left over and cannot be cleared: {error}",
                    base.display()
                )
            })?;
        }
        let work = base.join("work");
        let home = base.join("home");
        let tmp = base.join("tmp");
        for directory in [&work, &home, &tmp] {
            fs::create_dir_all(directory).map_err(|error| {
                format!(
                    "the workspace directory {} cannot be created: {error}",
                    directory.display()
                )
            })?;
        }
        Ok(Workspace {
            base,
            work,
            home,
            tmp,
        })
    }
}

/// One job's private corner of the data root, removed when the job ends.
#[derive(Debug, Clone)]
pub struct Workspace {
    base: PathBuf,
    work: PathBuf,
    home: PathBuf,
    tmp: PathBuf,
}

impl Workspace {
    /// The job's working directory, and the only one it is told about.
    pub fn work_dir(&self) -> &Path {
        &self.work
    }

    /// Everything under this job, workspace included.
    pub fn base_dir(&self) -> &Path {
        &self.base
    }

    /// The `HOME` the job is given. Inside the root, so a tool that insists
    /// on writing to the home directory writes here.
    pub fn home_dir(&self) -> &Path {
        &self.home
    }

    /// The `TMPDIR` the job is given, for the same reason.
    pub fn tmp_dir(&self) -> &Path {
        &self.tmp
    }

    /// How many bytes the job has written and not deleted. Apparent sizes,
    /// summed, following no symlink: the figure the disk budget is about is
    /// what the job put there, not how the filesystem stores it.
    pub fn size_bytes(&self) -> u64 {
        directory_size(&self.base)
    }

    /// Remove the workspace. A failure is reported to the caller rather than
    /// swallowed: a workspace that will not go away is a disk filling up.
    pub fn remove(&self) -> io::Result<()> {
        match fs::remove_dir_all(&self.base) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

/// Sum the apparent size of every regular file under a directory.
fn directory_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            // DirEntry::metadata does not follow symlinks, so a link to a
            // huge file outside the root counts as the link it is, and a
            // link loop cannot be walked.
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

/// A job's id as one directory name.
///
/// Ids are opaque (`JobId`), and an opaque string reaching the filesystem is
/// exactly how `../..` ends up in a path. So the name is built from the
/// characters that are safe and nothing else, and an id that has none of them
/// is refused naming itself.
fn directory_name(job_id: &JobId) -> Result<String, String> {
    let id = job_id.as_str();
    let name: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name.chars().all(|c| c == '_') {
        return Err(format!(
            "the job id {id:?} gives no usable directory name inside the data root"
        ));
    }
    Ok(name)
}

fn non_empty_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    #[test]
    fn a_fresh_root_is_created_and_holds_a_jobs_directory() {
        let temp = TempDir::new("root-fresh");
        let root = DataRoot::open(temp.path().join("state")).expect("a fresh root is usable");
        assert!(root.path().join(JOBS).is_dir());
        assert!(
            root.path().is_absolute(),
            "a root is canonical: {}",
            root.path().display()
        );
    }

    /// E-45's refusal: every reason names the path, because that is the only
    /// thing worth saying about a root that cannot be used.
    #[test]
    fn a_root_that_is_a_file_is_refused_naming_it() {
        let temp = TempDir::new("root-file");
        let file = temp.path().join("not-a-directory");
        fs::write(&file, b"i am a file").expect("the fixture writes");
        let error = DataRoot::open(&file).expect_err("a file is not a data root");
        assert!(
            error.contains(&file.display().to_string()),
            "the refusal must name the path: {error}"
        );
    }

    #[test]
    fn a_root_that_cannot_be_created_is_refused_naming_it() {
        let temp = TempDir::new("root-under-file");
        let file = temp.path().join("blocker");
        fs::write(&file, b"i am a file").expect("the fixture writes");
        let under = file.join("root");
        let error = DataRoot::open(&under).expect_err("there is a file in the way");
        assert!(
            error.contains(&under.display().to_string()),
            "the refusal must name the path: {error}"
        );
    }

    #[test]
    fn an_empty_root_path_is_refused() {
        let error = DataRoot::open("").expect_err("an empty path is not a root");
        assert!(error.contains("empty"), "{error}");
    }

    #[test]
    fn the_filesystem_root_is_not_a_data_root() {
        let error = DataRoot::open("/").expect_err("a runner may not own /");
        assert!(error.contains("filesystem root"), "{error}");
    }

    /// The gate every derived path goes through. A path that would leave the
    /// root is refused, and the refusal names it.
    #[test]
    fn a_path_leaving_the_root_is_refused_naming_it() {
        let temp = TempDir::new("resolve");
        let root = DataRoot::open(temp.path()).expect("usable");
        for escape in ["../etc/passwd", "jobs/../../etc/passwd", "/etc/passwd"] {
            let error = root
                .resolve(escape)
                .expect_err("an escaping path must be refused");
            assert!(
                error.contains(escape) && error.contains(&root.path().display().to_string()),
                "the refusal must name both the path and the root: {error}"
            );
        }
        let inside = root
            .resolve("jobs/j1/work")
            .expect("a path inside resolves");
        assert!(inside.starts_with(root.path()));
    }

    /// A job id is opaque, so it is treated as hostile: an id shaped like a
    /// path cannot reach outside the root through the workspace.
    #[test]
    fn a_job_id_shaped_like_a_path_cannot_escape_the_root() {
        let temp = TempDir::new("hostile-id");
        let root = DataRoot::open(temp.path()).expect("usable");
        let workspace = root
            .open_workspace(&JobId::new("../../escaped"))
            .expect("a hostile id is sanitised, not fatal");
        assert!(
            workspace.base_dir().starts_with(root.path()),
            "{} escaped the root",
            workspace.base_dir().display()
        );
        let error = directory_name(&JobId::new("///")).expect_err("no usable name");
        assert!(error.contains("///"), "{error}");
    }

    #[test]
    fn a_workspace_is_empty_three_directories_and_goes_away_again() {
        let temp = TempDir::new("workspace");
        let root = DataRoot::open(temp.path()).expect("usable");
        let job = JobId::new("j7");
        let workspace = root.open_workspace(&job).expect("made");
        for directory in [
            workspace.work_dir(),
            workspace.home_dir(),
            workspace.tmp_dir(),
        ] {
            assert!(directory.is_dir(), "{}", directory.display());
            assert!(directory.starts_with(root.path()));
        }
        assert_eq!(workspace.size_bytes(), 0, "a fresh workspace is empty");

        fs::write(workspace.work_dir().join("output"), vec![0u8; 5_000]).expect("the job writes");
        assert!(workspace.size_bytes() >= 5_000);

        // A second workspace for the same id starts empty again.
        let again = root.open_workspace(&job).expect("made again");
        assert_eq!(again.size_bytes(), 0);

        again.remove().expect("removed");
        assert!(!again.base_dir().exists());
        again.remove().expect("removing twice is not a failure");
    }
}
