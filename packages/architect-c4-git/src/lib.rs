//! GitPort adapter (gix) — ADR fixation commits, no push.

use architect_c4_domain::ports::GitPort;
use architect_c4_domain::DomainError;

fn git_err(s: impl Into<String>) -> DomainError {
    DomainError::Message(s.into())
}

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// gix-backed adapter — never shells out to `git`.
pub struct GixGitAdapter;

impl GixGitAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GixGitAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GitPort for GixGitAdapter {
    fn init_bare(&self, path: &Path) -> Result<PathBuf, DomainError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| git_err(e.to_string()))?;
        }
        let repo = gix::init_bare(path).map_err(|e| git_err(e.to_string()))?;
        // Seed empty commit on refs/heads/main so add_worktree can peel HEAD.
        seed_empty_main(&repo)?;
        Ok(repo.path().to_owned())
    }

    fn add_worktree(
        &self,
        bare: &Path,
        worktree_path: &Path,
        ref_name: &str,
    ) -> Result<PathBuf, DomainError> {
        let bare = bare.canonicalize().unwrap_or_else(|_| bare.to_path_buf());
        let repo = gix::open(&bare).map_err(|e| git_err(format!("open bare: {e}")))?;

        if worktree_path.exists()
            && worktree_path
                .read_dir()
                .map(|mut d| d.next().is_some())
                .unwrap_or(false)
        {
            return Err(git_err(format!(
                "worktree path not empty: {}",
                worktree_path.display()
            )));
        }
        fs::create_dir_all(worktree_path).map_err(|e| git_err(e.to_string()))?;

        let name = worktree_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| git_err("invalid worktree path"))?;

        let wt_git_dir = bare.join("worktrees").join(name);
        fs::create_dir_all(&wt_git_dir).map_err(|e| git_err(e.to_string()))?;

        // Resolve ref → object id (default refs/heads/main via HEAD)
        let branch_ref = normalize_branch_ref(ref_name);
        let mut head = repo
            .find_reference(&branch_ref)
            .or_else(|_| repo.find_reference("HEAD"))
            .map_err(|e| git_err(format!("resolve ref {ref_name}: {e}")))?;
        let id = head
            .peel_to_id_in_place()
            .map_err(|e| git_err(format!("peel: {e}")))?;
        let commit = repo
            .find_object(id)
            .map_err(|e| git_err(format!("object: {e}")))?
            .peel_to_commit()
            .map_err(|e| git_err(format!("commit: {e}")))?;
        let tree_id = commit.tree_id().map_err(|e| git_err(e.to_string()))?;

        // Linked worktree metadata (git worktree layout, no CLI)
        let abs_wt = worktree_path
            .canonicalize()
            .unwrap_or_else(|_| worktree_path.to_path_buf());
        write_file(
            &wt_git_dir.join("gitdir"),
            format!("{}\n", abs_wt.join(".git").display()),
        )?;
        write_file(&wt_git_dir.join("commondir"), "../..\n")?;
        // Symbolic HEAD so commits update the shared branch on the bare repo.
        write_file(&wt_git_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;

        write_file(
            &worktree_path.join(".git"),
            format!("gitdir: {}\n", wt_git_dir.display()),
        )?;

        // Materialize files from tree (best-effort checkout)
        checkout_tree_to(&repo, tree_id.detach(), worktree_path)?;

        Ok(abs_wt)
    }

    fn commit(
        &self,
        worktree_path: &Path,
        message: &str,
        paths: &[String],
    ) -> Result<String, DomainError> {
        let repo = gix::open(worktree_path).map_err(|e| git_err(format!("open worktree: {e}")))?;

        // For v1: if paths empty, commit is a no-op message commit on current tree;
        // otherwise require files exist and rebuild tree from worktree files listed.
        if paths.is_empty() {
            return Err(git_err(
                "commit requires at least one path in v1 (stage explicit paths)",
            ));
        }

        for p in paths {
            let full = worktree_path.join(p);
            if !full.is_file() {
                return Err(git_err(format!("missing file for commit: {p}")));
            }
        }

        // Build a simple tree from listed files (flat paths only in v1)
        let mut entries: Vec<(String, gix::ObjectId)> = Vec::new();
        for p in paths {
            let bytes = fs::read(worktree_path.join(p)).map_err(|e| git_err(e.to_string()))?;
            let blob_id = repo
                .write_blob(&bytes)
                .map_err(|e| git_err(format!("write blob: {e}")))?
                .detach();
            let name = Path::new(p)
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| git_err("bad path"))?
                .to_string();
            // Keep relative path components for nested files
            entries.push((p.replace('\\', "/"), blob_id));
            let _ = name;
        }

        let tree_id = write_flat_tree(&repo, &entries)?;
        let author = gix::actor::Signature {
            name: "architect-c4-git".into(),
            email: "architect-c4-git@localhost".into(),
            time: gix::date::Time::now_local_or_utc(),
        };
        let mut author_buf = gix_date::parse::TimeBuf::default();
        let mut committer_buf = gix_date::parse::TimeBuf::default();
        let author_ref = author.to_ref(&mut author_buf);
        let committer_ref = author.to_ref(&mut committer_buf);

        let parent = repo.head_id().ok().map(|id| id.detach());
        let mut parents: Vec<gix::ObjectId> = Vec::new();
        if let Some(p) = parent {
            parents.push(p);
        }

        let commit_id = repo
            .commit_as(
                committer_ref,
                author_ref,
                "HEAD",
                message,
                tree_id,
                parents.iter().copied(),
            )
            .map_err(|e| git_err(format!("commit: {e}")))?;

        Ok(commit_id.to_string())
    }
}

fn normalize_branch_ref(ref_name: &str) -> String {
    if ref_name == "HEAD" || ref_name.is_empty() {
        "refs/heads/main".to_string()
    } else if ref_name.starts_with("refs/") {
        ref_name.to_string()
    } else {
        format!("refs/heads/{ref_name}")
    }
}

/// Create an empty-tree commit on `refs/heads/main` and point HEAD at it.
fn seed_empty_main(repo: &gix::Repository) -> Result<(), DomainError> {
    let tree = gix::objs::Tree::empty();
    let tree_id = repo
        .write_object(&tree)
        .map_err(|e| git_err(format!("write empty tree: {e}")))?
        .detach();

    let author = gix::actor::Signature {
        name: "architect-c4-git".into(),
        email: "architect-c4-git@localhost".into(),
        time: gix::date::Time::now_local_or_utc(),
    };
    let mut author_buf = gix_date::parse::TimeBuf::default();
    let mut committer_buf = gix_date::parse::TimeBuf::default();
    let author_ref = author.to_ref(&mut author_buf);
    let committer_ref = author.to_ref(&mut committer_buf);

    let parents: Vec<gix::ObjectId> = Vec::new();
    repo.commit_as(
        committer_ref,
        author_ref,
        "refs/heads/main",
        "initial empty commit",
        tree_id,
        parents.iter().copied(),
    )
    .map_err(|e| git_err(format!("seed commit: {e}")))?;

    // Ensure symbolic HEAD → refs/heads/main (gix init_bare usually does this).
    let head_path = repo.path().join("HEAD");
    if !head_path.exists() {
        write_file(&head_path, "ref: refs/heads/main\n")?;
    }
    Ok(())
}

fn write_file(path: &Path, content: impl AsRef<[u8]>) -> Result<(), DomainError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| git_err(e.to_string()))?;
    }
    let mut f = fs::File::create(path).map_err(|e| git_err(e.to_string()))?;
    f.write_all(content.as_ref())
        .map_err(|e| git_err(e.to_string()))?;
    Ok(())
}

fn checkout_tree_to(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    dest: &Path,
) -> Result<(), DomainError> {
    // Recursively checkout tree entries as files/dirs (simple, no filters).
    fn walk(
        repo: &gix::Repository,
        tree_id: gix::ObjectId,
        base: &Path,
    ) -> Result<(), DomainError> {
        let tree = repo
            .find_object(tree_id)
            .map_err(|e| git_err(e.to_string()))?
            .peel_to_tree()
            .map_err(|e| git_err(e.to_string()))?;
        for entry in tree.iter() {
            let entry = entry.map_err(|e| git_err(e.to_string()))?;
            let name = entry.filename().to_string();
            let path = base.join(&name);
            let mode = entry.mode();
            let oid = entry.oid().to_owned();
            if mode.is_tree() {
                fs::create_dir_all(&path).map_err(|e| git_err(e.to_string()))?;
                walk(repo, oid, &path)?;
            } else if mode.is_blob() || mode.is_executable() {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|e| git_err(e.to_string()))?;
                }
                let obj = repo.find_object(oid).map_err(|e| git_err(e.to_string()))?;
                let blob = obj.try_into_blob().map_err(|e| git_err(e.to_string()))?;
                fs::write(&path, blob.data.as_slice()).map_err(|e| git_err(e.to_string()))?;
                #[cfg(unix)]
                if mode.is_executable() {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = fs::metadata(&path)
                        .map_err(|e| git_err(e.to_string()))?
                        .permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(&path, perms).map_err(|e| git_err(e.to_string()))?;
                }
            }
        }
        Ok(())
    }
    walk(repo, tree_id, dest)
}

fn write_flat_tree(
    repo: &gix::Repository,
    entries: &[(String, gix::ObjectId)],
) -> Result<gix::ObjectId, DomainError> {
    // Nested path support: build tree recursively from path components.
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Node {
        files: BTreeMap<String, gix::ObjectId>,
        dirs: BTreeMap<String, Node>,
    }

    let mut root = Node::default();
    for (path, oid) in entries {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            continue;
        }
        let mut node = &mut root;
        for part in &parts[..parts.len() - 1] {
            node = node.dirs.entry((*part).to_string()).or_default();
        }
        node.files.insert(parts[parts.len() - 1].to_string(), *oid);
    }

    fn write_node(repo: &gix::Repository, node: &Node) -> Result<gix::ObjectId, DomainError> {
        let mut tree = gix::objs::Tree::empty();
        for (name, oid) in &node.files {
            tree.entries.push(gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: name.as_str().into(),
                oid: *oid,
            });
        }
        for (name, child) in &node.dirs {
            let child_id = write_node(repo, child)?;
            tree.entries.push(gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Tree.into(),
                filename: name.as_str().into(),
                oid: child_id,
            });
        }
        tree.entries.sort_by(|a, b| a.filename.cmp(&b.filename));
        Ok(repo
            .write_object(&tree)
            .map_err(|e| git_err(format!("write tree: {e}")))?
            .detach())
    }

    write_node(repo, &root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn init_worktree_commit_roundtrip() {
        let dir = tempdir().unwrap();
        let git = GixGitAdapter::new();
        let bare = git.init_bare(&dir.path().join("demo.git")).unwrap();
        let wt = git
            .add_worktree(&bare, &dir.path().join("wt"), "main")
            .unwrap();
        fs::write(wt.join("presentation.ir.json"), b"{\"title\":\"T\"}").unwrap();
        let cid = git
            .commit(&wt, "add ir", &["presentation.ir.json".into()])
            .unwrap();
        assert!(cid.len() >= 7);

        let wt2 = git
            .add_worktree(&bare, &dir.path().join("wt2"), "refs/heads/main")
            .unwrap();
        assert!(wt2.join("presentation.ir.json").is_file());
    }

    #[test]
    fn commit_errors_and_nested_paths() {
        let dir = tempdir().unwrap();
        let git = GixGitAdapter::new();
        let bare = git.init_bare(&dir.path().join("b.git")).unwrap();
        let wt = git.add_worktree(&bare, &dir.path().join("wt"), "").unwrap();
        assert!(git.commit(&wt, "x", &[]).is_err());
        assert!(git.commit(&wt, "x", &["missing.txt".into()]).is_err());

        fs::create_dir_all(wt.join("sub")).unwrap();
        fs::write(wt.join("sub/file.txt"), b"hi").unwrap();
        let cid = git.commit(&wt, "nested", &["sub/file.txt".into()]).unwrap();
        assert!(!cid.is_empty());
    }

    #[test]
    fn reject_non_empty_worktree() {
        let dir = tempdir().unwrap();
        let git = GixGitAdapter::new();
        let bare = git.init_bare(&dir.path().join("b.git")).unwrap();
        let wt = dir.path().join("wt");
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join("x"), b"1").unwrap();
        assert!(git.add_worktree(&bare, &wt, "main").is_err());
    }

    #[test]
    fn normalize_branch_ref_variants() {
        assert_eq!(normalize_branch_ref("HEAD"), "refs/heads/main");
        assert_eq!(normalize_branch_ref(""), "refs/heads/main");
        assert_eq!(normalize_branch_ref("refs/heads/dev"), "refs/heads/dev");
        assert_eq!(normalize_branch_ref("feature"), "refs/heads/feature");
    }

    #[test]
    fn init_bare_fails_when_parent_is_file() {
        let dir = tempdir().unwrap();
        let blocker = dir.path().join("file");
        fs::write(&blocker, b"x").unwrap();
        let git = GixGitAdapter::new();
        assert!(git.init_bare(&blocker.join("repo.git")).is_err());
    }

    #[test]
    fn checkout_nested_and_executable_blob() {
        let dir = tempdir().unwrap();
        let git = GixGitAdapter::new();
        let bare = git.init_bare(&dir.path().join("b.git")).unwrap();
        let wt = git
            .add_worktree(&bare, &dir.path().join("wt"), "main")
            .unwrap();
        fs::create_dir_all(wt.join("bin")).unwrap();
        fs::write(wt.join("bin/tool"), b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(wt.join("bin/tool")).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(wt.join("bin/tool"), perms).unwrap();
        }
        // Commit as regular blob then rewrite tree entry as executable via gix for checkout coverage.
        git.commit(&wt, "tool", &["bin/tool".into()]).unwrap();

        // Build a commit with an executable-mode tree entry and checkout into wt3.
        let repo = gix::open(&bare).unwrap();
        let bytes = b"#!/bin/sh\necho hi\n";
        let blob = repo.write_blob(bytes).unwrap().detach();
        let mut tree = gix::objs::Tree::empty();
        tree.entries.push(gix::objs::tree::Entry {
            mode: gix::objs::tree::EntryKind::BlobExecutable.into(),
            filename: "run.sh".into(),
            oid: blob,
        });
        let tree_id = repo.write_object(&tree).unwrap().detach();
        let author = gix::actor::Signature {
            name: "t".into(),
            email: "t@t".into(),
            time: gix::date::Time::now_local_or_utc(),
        };
        let mut ab = gix_date::parse::TimeBuf::default();
        let mut cb = gix_date::parse::TimeBuf::default();
        let parent = repo.head_id().unwrap().detach();
        repo.commit_as(
            author.to_ref(&mut cb),
            author.to_ref(&mut ab),
            "refs/heads/main",
            "exec",
            tree_id,
            std::iter::once(parent),
        )
        .unwrap();

        let wt3 = git
            .add_worktree(&bare, &dir.path().join("wt3"), "main")
            .unwrap();
        assert!(wt3.join("run.sh").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(wt3.join("run.sh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn write_flat_tree_skips_empty_path_components() {
        let dir = tempdir().unwrap();
        let git = GixGitAdapter::new();
        let bare = git.init_bare(&dir.path().join("b.git")).unwrap();
        let repo = gix::open(&bare).unwrap();
        let blob = repo.write_blob(b"x").unwrap().detach();
        let id = write_flat_tree(&repo, &[("/".into(), blob), ("ok.txt".into(), blob)]).unwrap();
        let _ = id;
    }

    #[test]
    fn checkout_preserves_nested_tree() {
        let dir = tempdir().unwrap();
        let git = GixGitAdapter::new();
        let bare = git.init_bare(&dir.path().join("b.git")).unwrap();
        let wt = git
            .add_worktree(&bare, &dir.path().join("wt"), "main")
            .unwrap();
        fs::create_dir_all(wt.join("a/b")).unwrap();
        fs::write(wt.join("a/b/c.txt"), b"nested").unwrap();
        git.commit(&wt, "nest", &["a/b/c.txt".into()]).unwrap();
        let wt2 = git
            .add_worktree(&bare, &dir.path().join("wt-nest"), "main")
            .unwrap();
        assert_eq!(fs::read_to_string(wt2.join("a/b/c.txt")).unwrap(), "nested");
    }

    #[test]
    fn git_error_display() {
        assert_eq!(git_err("boom").to_string(), "boom");
    }

    #[test]
    fn default_adapter_and_commit_after_seed() {
        let git = GixGitAdapter::new();
        let dir = tempdir().unwrap();
        let bare = git.init_bare(&dir.path().join("d.git")).unwrap();
        let wt = git
            .add_worktree(&bare, &dir.path().join("w"), "HEAD")
            .unwrap();
        fs::write(wt.join("a.txt"), b"1").unwrap();
        fs::write(wt.join("b.txt"), b"2").unwrap();
        let c1 = git.commit(&wt, "a", &["a.txt".into()]).unwrap();
        let c2 = git
            .commit(&wt, "b", &["a.txt".into(), "b.txt".into()])
            .unwrap();
        assert_ne!(c1, c2);
        let wt2 = git
            .add_worktree(&bare, &dir.path().join("w2"), "refs/heads/main")
            .unwrap();
        assert_eq!(fs::read_to_string(wt2.join("b.txt")).unwrap(), "2");
    }
}
