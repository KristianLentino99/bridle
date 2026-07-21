//! Skills sync — mirror `~/Bridle/skills/` into each harness's skills directory.
//!
//! A "skill" is a top-level directory or symlink inside the skills folder.
//! Harness-specific system entries (names starting with `.`) are never created,
//! removed, or overwritten by bridle.

use crate::harness::HarnessSpec;
use crate::platform::Platform;
use crate::sync::SyncState;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Result of syncing skills to one harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsSyncReport {
    pub harness_id: &'static str,
    pub action: SkillsSyncAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillsSyncAction {
    /// Skills were installed or updated on the harness.
    Updated {
        installed: Vec<String>,
        forced: bool,
    },
    /// Harness already mirrors the canonical skills directory.
    AlreadyInSync,
    /// Harness is not installed.
    NotInstalled,
    /// Harness has no skills directory configured.
    NoSkillsDir,
    /// One or more harness skill entries differ from canonical and force was not set.
    Drift { skills: Vec<String> },
    /// An error occurred.
    Error(String),
}

/// Canonical skills directory inside the bridle home.
pub fn master_skills_dir(bridle_home: &Path) -> PathBuf {
    bridle_home.join("skills")
}

/// Resolve the harness-specific skills directory, if any.
pub fn harness_skills_dir(spec: &HarnessSpec, platform: Platform) -> Option<PathBuf> {
    spec.skills_dir.map(|dir| spec.base_dir(platform).join(dir))
}

/// List top-level skill names in a directory.
///
/// Only includes directories and directory-symlinks; hidden entries (starting with `.`)
/// are ignored because those are typically harness system directories.
pub fn list_skill_names(dir: &Path) -> io::Result<Vec<String>> {
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut names = vec![];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let meta = entry.metadata()?;
        let is_dir = meta.is_dir();
        let is_dir_symlink = meta.file_type().is_symlink() && fs::metadata(entry.path())?.is_dir();
        if is_dir || is_dir_symlink {
            names.push(name.into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// Recursively hash a directory's contents into a deterministic SHA-256 hex string.
///
/// The hash includes every file's relative path and contents, sorted lexicographically.
/// Empty directories hash to the empty string.
pub fn hash_dir(dir: &Path) -> io::Result<String> {
    let mut entries: Vec<(PathBuf, Vec<u8>)> = vec![];
    collect_dir_entries(dir, dir, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel_path, contents) in entries {
        hasher.update(rel_path.as_os_str().as_encoded_bytes());
        hasher.update(b"\0");
        hasher.update(&contents);
        hasher.update(b"\0");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_dir_entries(
    root: &Path,
    current: &Path,
    out: &mut Vec<(PathBuf, Vec<u8>)>,
) -> io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.file_type().is_symlink() {
            continue; // don't follow symlinks when hashing
        }
        if meta.is_dir() {
            collect_dir_entries(root, &path, out)?;
        } else {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let mut file = fs::File::open(&path)?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;
            out.push((rel, contents));
        }
    }
    Ok(())
}

/// Hash of the canonical skills directory state suitable for drift detection.
///
/// Combines skill names with a content hash of each skill directory.
pub fn hash_skills_state(skills_dir: &Path) -> io::Result<String> {
    let names = list_skill_names(skills_dir)?;
    if names.is_empty() {
        return Ok(String::new());
    }

    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for name in names {
        let skill_path = skills_dir.join(&name);
        let hash = if skill_path.is_symlink() {
            // For symlinked skills, hash the target path so re-pointing is detected.
            match fs::read_link(&skill_path) {
                Ok(target) => hash_bytes(target.as_os_str().as_encoded_bytes()),
                Err(_) => String::new(),
            }
        } else {
            hash_dir(&skill_path).unwrap_or_default()
        };
        map.insert(name, hash);
    }

    let json = serde_json::to_string(&map).unwrap_or_default();
    Ok(hash_bytes(json.as_bytes()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Check whether a harness skill entry is equivalent to the canonical skill.
fn skill_is_equivalent(master_skill: &Path, harness_skill: &Path) -> bool {
    if !harness_skill.exists() && !harness_skill.is_symlink() {
        return false;
    }

    if harness_skill.is_symlink() {
        return match fs::read_link(harness_skill) {
            Ok(target) => target == master_skill,
            Err(_) => false,
        };
    }

    if master_skill.is_symlink() || harness_skill.is_symlink() {
        // One is a real dir and the other is a symlink: not equivalent.
        return false;
    }

    if master_skill.is_dir() && harness_skill.is_dir() {
        return match (hash_dir(master_skill), hash_dir(harness_skill)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
    }

    false
}

/// Install a skill from the canonical directory into a harness.
///
/// On Unix, creates a symlink to the canonical skill so future edits are reflected
/// automatically. On Windows, tries a directory symlink and falls back to a recursive
/// copy if permissions are insufficient.
pub fn install_skill(master_skill: &Path, harness_skill: &Path) -> io::Result<()> {
    // Remove whatever is currently at the destination.
    if harness_skill.exists() || harness_skill.is_symlink() {
        let meta = fs::symlink_metadata(harness_skill)?;
        if meta.is_dir() {
            fs::remove_dir_all(harness_skill)?;
        } else {
            fs::remove_file(harness_skill)?;
        }
    }

    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(master_skill, harness_skill).is_ok() {
            return Ok(());
        }
    }

    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(master_skill, harness_skill).is_ok() {
            return Ok(());
        }
    }

    // Fallback: recursive copy.
    copy_dir_all(master_skill, harness_skill)
}

fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let meta = entry.metadata()?;
        if meta.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Sync skills to a single harness.
///
/// When `dry_run` is `true`, no files are written and `state` is not mutated,
/// but the returned report reflects what would happen.
pub fn sync_skills_one(
    spec: &'static HarnessSpec,
    master_dir: &Path,
    state: &mut SyncState,
    platform: Platform,
    force: bool,
    dry_run: bool,
) -> SkillsSyncReport {
    let base = spec.base_dir(platform);
    if !base.exists() {
        return SkillsSyncReport {
            harness_id: spec.id,
            action: SkillsSyncAction::NotInstalled,
        };
    }

    let harness_skills = match harness_skills_dir(spec, platform) {
        Some(dir) => dir,
        None => {
            return SkillsSyncReport {
                harness_id: spec.id,
                action: SkillsSyncAction::NoSkillsDir,
            }
        }
    };

    let master_names = match list_skill_names(master_dir) {
        Ok(names) => names,
        Err(e) => {
            return SkillsSyncReport {
                harness_id: spec.id,
                action: SkillsSyncAction::Error(format!("cannot read canonical skills: {e}")),
            }
        }
    };

    let master_hash = match hash_skills_state(master_dir) {
        Ok(h) => h,
        Err(e) => {
            return SkillsSyncReport {
                harness_id: spec.id,
                action: SkillsSyncAction::Error(format!("cannot hash canonical skills: {e}")),
            }
        }
    };

    // Determine which skills need installing and which are drifted.
    let mut to_install: Vec<String> = vec![];
    let mut drifted: Vec<String> = vec![];

    for name in &master_names {
        let master_skill = master_dir.join(name);
        let harness_skill = harness_skills.join(name);

        if skill_is_equivalent(&master_skill, &harness_skill) {
            continue;
        }

        if harness_skill.exists() || harness_skill.is_symlink() {
            drifted.push(name.clone());
        } else {
            to_install.push(name.clone());
        }
    }

    if to_install.is_empty() && drifted.is_empty() {
        if !dry_run {
            state
                .last_skill_hashes
                .insert(spec.id.to_string(), master_hash);
        }
        return SkillsSyncReport {
            harness_id: spec.id,
            action: SkillsSyncAction::AlreadyInSync,
        };
    }

    if !drifted.is_empty() && !force {
        return SkillsSyncReport {
            harness_id: spec.id,
            action: SkillsSyncAction::Drift { skills: drifted },
        };
    }

    let has_drift = !drifted.is_empty();
    let mut installed = to_install;
    installed.extend(drifted); // drifted entries will be overwritten

    if !dry_run {
        // Ensure harness skills dir exists.
        if let Err(e) = fs::create_dir_all(&harness_skills) {
            return SkillsSyncReport {
                harness_id: spec.id,
                action: SkillsSyncAction::Error(format!("cannot create skills dir: {e}")),
            };
        }

        for name in &installed {
            let master_skill = master_dir.join(name);
            let harness_skill = harness_skills.join(name);
            if let Err(e) = install_skill(&master_skill, &harness_skill) {
                return SkillsSyncReport {
                    harness_id: spec.id,
                    action: SkillsSyncAction::Error(format!("failed to install {name}: {e}")),
                };
            }
        }

        state
            .last_skill_hashes
            .insert(spec.id.to_string(), master_hash);
    }

    SkillsSyncReport {
        harness_id: spec.id,
        action: SkillsSyncAction::Updated {
            installed,
            forced: has_drift,
        },
    }
}

/// Sync skills to all harnesses that support them.
pub fn sync_skills_all(
    master_dir: &Path,
    state: &mut SyncState,
    platform: Platform,
    force: bool,
    dry_run: bool,
) -> Vec<SkillsSyncReport> {
    crate::harness::all()
        .iter()
        .map(|spec| sync_skills_one(spec, master_dir, state, platform, force, dry_run))
        .collect()
}

// ── Skills import (for `bridle import`) ────────────────────────────

/// Result of importing skills into the canonical bridle skills directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillsImportReport {
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<(String, String)>,
}

/// Priority of a skill source directory. Used to resolve collisions when
/// the same skill name appears in multiple source locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillSourcePriority {
    Default = 0,
    User = 1,
    Project = 2,
}

impl fmt::Display for SkillSourcePriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkillSourcePriority::Project => write!(f, "project"),
            SkillSourcePriority::User => write!(f, "user"),
            SkillSourcePriority::Default => write!(f, "default"),
        }
    }
}

/// A directory containing skills to import, with its priority level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSource {
    pub path: PathBuf,
    pub priority: SkillSourcePriority,
}

/// Record of a collision encountered during multi-source import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCollision {
    pub name: String,
    pub chosen: SkillSource,
    pub skipped: Vec<SkillSource>,
}

/// Full report from a multi-source skills import.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MultiSourceImportReport {
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub collisions: Vec<SkillCollision>,
    pub errors: Vec<(String, String)>,
}

/// Import skills from a source directory into the canonical bridle skills directory.
///
/// By default, skills are copied (not symlinked) so `~/Bridle/skills/` becomes the
/// new canonical source.
///
/// - `force`: overwrite existing skills unconditionally
/// - `link`: create symlinks instead of copies, so the source stays canonical
///   and updates propagate automatically
/// - `update`: overwrite only skills whose source content has changed since the
///   last import
pub fn import_skills(
    source: &Path,
    target: &Path,
    force: bool,
    link: bool,
    update: bool,
) -> io::Result<SkillsImportReport> {
    fs::create_dir_all(target)?;

    let source_names = list_skill_names(source)?;
    let mut report = SkillsImportReport::default();

    for name in source_names {
        let source_skill = source.join(&name);
        let target_skill = target.join(&name);

        // Resolve symlinks in the source so we compare/install the actual skill contents.
        let source_skill = fs::canonicalize(&source_skill).unwrap_or_else(|_| source_skill.clone());

        let action = if target_skill.exists() || target_skill.is_symlink() {
            determine_import_action(&source_skill, &target_skill, force, link, update)
        } else {
            ImportAction::Install
        };

        match action {
            ImportAction::Skip => {
                report.skipped.push(name);
                continue;
            }
            ImportAction::RemoveAndInstall => {
                if let Err(e) = remove_skill_entry(&target_skill) {
                    report.errors.push((name.clone(), e.to_string()));
                    continue;
                }
            }
            ImportAction::Install => {}
        }

        let result = if link {
            create_symlink(&source_skill, &target_skill)
        } else {
            copy_dir_all(&source_skill, &target_skill)
        };

        if let Err(e) = result {
            report.errors.push((name.clone(), e.to_string()));
        } else {
            report.imported.push(name);
        }
    }

    Ok(report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportAction {
    Install,
    RemoveAndInstall,
    Skip,
}

fn determine_import_action(
    source_skill: &Path,
    target_skill: &Path,
    force: bool,
    link: bool,
    update: bool,
) -> ImportAction {
    if force {
        return ImportAction::RemoveAndInstall;
    }

    // If the target already points to the source, nothing to do.
    if target_skill.is_symlink()
        && fs::read_link(target_skill).ok().as_deref() == Some(source_skill)
    {
        return ImportAction::Skip;
    }

    if link {
        // In link mode without force/update, leave existing entries alone.
        ImportAction::Skip
    } else if update {
        if target_skill.is_symlink() {
            // Symlink to a different source -> replace with a copy.
            ImportAction::RemoveAndInstall
        } else {
            // Compare content hashes for copied skills.
            match (hash_dir(source_skill), hash_dir(target_skill)) {
                (Ok(a), Ok(b)) if a == b => ImportAction::Skip,
                _ => ImportAction::RemoveAndInstall,
            }
        }
    } else {
        ImportAction::Skip
    }
}

fn remove_skill_entry(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn create_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(src, dst)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = src;
        let _ = dst;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symlinks are not supported on this platform",
        ))
    }
}

// ── Multi-source skills import ────────────────────────────────────────

/// Discover all available skill source directories.
///
/// Sources are discovered in this order:
/// 1. Project `.agents/skills` — walking up from cwd (priority: Project)
/// 2. User harness skill directories that exist (priority: User)
/// 3. `~/.agents/skills` (priority: Default)
pub fn discover_skill_sources(platform: Platform, cwd: &Path) -> Vec<SkillSource> {
    let mut sources: Vec<SkillSource> = vec![];

    // 1. Project `.agents/skills/` — walk up from cwd
    let mut current = Some(cwd.to_path_buf());
    while let Some(dir) = current {
        let agents_skills = dir.join(".agents").join("skills");
        if agents_skills.is_dir() {
            sources.push(SkillSource {
                path: agents_skills,
                priority: SkillSourcePriority::Project,
            });
            break; // only the closest project wins
        }
        current = dir.parent().map(Path::to_path_buf);
    }

    // 2. User harness skill directories
    for spec in crate::harness::all() {
        if spec.skills_dir.is_none() {
            continue;
        }
        let base = spec.base_dir(platform);
        let skills = base.join(spec.skills_dir.unwrap());
        if skills.is_dir() {
            // Skip if already covered by project source
            if sources.iter().any(|s| s.path == skills) {
                continue;
            }
            sources.push(SkillSource {
                path: skills,
                priority: SkillSourcePriority::User,
            });
        }
    }

    // 3. `~/.agents/skills/` (default legacy location)
    let default_skills = crate::platform::home_dir().join(".agents").join("skills");
    if default_skills.is_dir() {
        // Skip if already covered
        if !sources.iter().any(|s| s.path == default_skills) {
            sources.push(SkillSource {
                path: default_skills,
                priority: SkillSourcePriority::Default,
            });
        }
    }

    sources
}

/// Import skills from multiple source directories, resolving collisions
/// by source priority (project > user > default).
///
/// Same-named skills found in multiple sources are resolved automatically:
/// the highest-priority source wins and lower-priority ones are reported
/// as collisions.
pub fn import_skills_from_sources(
    sources: &[SkillSource],
    target: &Path,
    force: bool,
    link: bool,
    update: bool,
) -> io::Result<MultiSourceImportReport> {
    fs::create_dir_all(target)?;

    let mut report = MultiSourceImportReport::default();

    // Collect all skill names and their sources, sorted by priority.
    // Key: skill name, Value: list of (source, priority) pairs.
    let mut skill_map: BTreeMap<String, Vec<&SkillSource>> = BTreeMap::new();

    for source in sources {
        let names = list_skill_names(&source.path).unwrap_or_default();
        for name in names {
            skill_map.entry(name).or_default().push(source);
        }
    }

    for (name, srcs) in &skill_map {
        // Sort sources by descending priority so the winner is first.
        let mut sorted: Vec<&SkillSource> = srcs.to_vec();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.priority));

        let winner = sorted[0];
        let skipped: Vec<SkillSource> = sorted[1..].iter().map(|s| (*s).clone()).collect();

        if !skipped.is_empty() {
            report.collisions.push(SkillCollision {
                name: name.clone(),
                chosen: winner.clone(),
                skipped: skipped.clone(),
            });
        }

        let source_skill = winner.path.join(name);
        let source_skill = fs::canonicalize(&source_skill).unwrap_or_else(|_| source_skill.clone());
        let target_skill = target.join(name);

        let action = if target_skill.exists() || target_skill.is_symlink() {
            determine_import_action(&source_skill, &target_skill, force, link, update)
        } else {
            ImportAction::Install
        };

        match action {
            ImportAction::Skip => {
                report.skipped.push(name.clone());
            }
            ImportAction::RemoveAndInstall => {
                if let Err(e) = remove_skill_entry(&target_skill) {
                    report.errors.push((name.clone(), e.to_string()));
                    continue;
                }
                // fall through to Install
                if let Err(e) = install_skill_from_source(&source_skill, &target_skill, link) {
                    report.errors.push((name.clone(), e.to_string()));
                } else {
                    report.imported.push(name.clone());
                }
            }
            ImportAction::Install => {
                if let Err(e) = install_skill_from_source(&source_skill, &target_skill, link) {
                    report.errors.push((name.clone(), e.to_string()));
                } else {
                    report.imported.push(name.clone());
                }
            }
        }
    }

    Ok(report)
}

/// Install a skill from a source path into the target, creating a symlink
/// or copy based on the `link` flag.
fn install_skill_from_source(source: &Path, target: &Path, link: bool) -> io::Result<()> {
    if link {
        create_symlink(source, target)
    } else {
        copy_dir_all(source, target)
    }
}

// ── Skills removal (for `bridle remove`) ────────────────────────────

/// Remove a skill from the canonical bridle skills directory.
///
/// Returns `Ok(true)` if a skill was removed, `Ok(false)` if it did not exist.
pub fn remove_skill(skills_dir: &Path, name: &str) -> io::Result<bool> {
    let skill_path = skills_dir.join(name);
    if skill_path.exists() || skill_path.is_symlink() {
        remove_skill_entry(&skill_path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

// ── Skills status (for `bridle status`) ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsStatusReport {
    pub harness_id: &'static str,
    pub state: SkillsStatusState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillsStatusState {
    InSync,
    NotInstalled,
    NoSkillsDir,
    Missing {
        skills: Vec<String>,
    },
    Drifted {
        skills: Vec<String>,
    },
    Mixed {
        missing: Vec<String>,
        drifted: Vec<String>,
    },
    Error(String),
}

/// Compute the skills status for a single harness without modifying anything.
pub fn status_skills_one(
    spec: &'static HarnessSpec,
    master_dir: &Path,
    platform: Platform,
) -> SkillsStatusReport {
    let base = spec.base_dir(platform);
    if !base.exists() {
        return SkillsStatusReport {
            harness_id: spec.id,
            state: SkillsStatusState::NotInstalled,
        };
    }

    let harness_skills = match harness_skills_dir(spec, platform) {
        Some(dir) => dir,
        None => {
            return SkillsStatusReport {
                harness_id: spec.id,
                state: SkillsStatusState::NoSkillsDir,
            }
        }
    };

    let master_names = match list_skill_names(master_dir) {
        Ok(names) => names,
        Err(e) => {
            return SkillsStatusReport {
                harness_id: spec.id,
                state: SkillsStatusState::Error(format!("cannot read canonical skills: {e}")),
            }
        }
    };

    let mut missing: Vec<String> = vec![];
    let mut drifted: Vec<String> = vec![];

    for name in &master_names {
        let master_skill = master_dir.join(name);
        let harness_skill = harness_skills.join(name);

        if !harness_skill.exists() && !harness_skill.is_symlink() {
            missing.push(name.clone());
        } else if !skill_is_equivalent(&master_skill, &harness_skill) {
            drifted.push(name.clone());
        }
    }

    let state = if missing.is_empty() && drifted.is_empty() {
        SkillsStatusState::InSync
    } else if !missing.is_empty() && !drifted.is_empty() {
        SkillsStatusState::Mixed { missing, drifted }
    } else if !missing.is_empty() {
        SkillsStatusState::Missing { skills: missing }
    } else {
        SkillsStatusState::Drifted { skills: drifted }
    };

    SkillsStatusReport {
        harness_id: spec.id,
        state,
    }
}

/// Compute skills status for all harnesses.
pub fn status_skills_all(master_dir: &Path, platform: Platform) -> Vec<SkillsStatusReport> {
    crate::harness::all()
        .iter()
        .map(|spec| status_skills_one(spec, master_dir, platform))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{HarnessSpec, McpFormat};
    use crate::platform::Platform;
    use tempfile::TempDir;

    fn write_skill_file(dir: &Path, name: &str, rel_path: &str, content: &str) -> PathBuf {
        let skill_dir = dir.join(name);
        let file = skill_dir.join(rel_path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, content).unwrap();
        skill_dir
    }

    #[test]
    fn list_skill_names_ignores_hidden_and_files() {
        let tmp = TempDir::new().unwrap();
        let skills = tmp.path().join("skills");
        fs::create_dir(&skills).unwrap();
        fs::create_dir(skills.join("alpha")).unwrap();
        fs::create_dir(skills.join("beta")).unwrap();
        fs::create_dir(skills.join(".system")).unwrap();
        fs::write(skills.join("readme.txt"), "hello").unwrap();

        let names = list_skill_names(&skills).unwrap();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn hash_dir_detects_content_changes() {
        let tmp = TempDir::new().unwrap();
        let skill = tmp.path().join("skill");
        fs::create_dir(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "v1").unwrap();

        let h1 = hash_dir(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "v2").unwrap();
        let h2 = hash_dir(&skill).unwrap();

        assert_ne!(h1, h2);
    }

    #[test]
    fn skill_is_equivalent_for_symlink() {
        let tmp = TempDir::new().unwrap();
        let master = tmp.path().join("master").join("skill");
        fs::create_dir_all(&master).unwrap();
        fs::write(master.join("SKILL.md"), "x").unwrap();

        let harness = tmp.path().join("harness").join("skill");
        fs::create_dir_all(harness.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&master, &harness).unwrap();
        #[cfg(not(unix))]
        copy_dir_all(&master, &harness).unwrap();

        assert!(skill_is_equivalent(&master, &harness));
    }

    #[test]
    fn sync_skills_installs_missing_skill() {
        let tmp = TempDir::new().unwrap();
        let bridle_home = tmp.path().join("Bridle");
        let master_skills = bridle_home.join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".pi").join("agent");
        fs::create_dir_all(&harness_base).unwrap();
        let spec = HarnessSpec {
            id: "pi",
            name: "Pi",
            mcp_format: McpFormat::JsonWithImports,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: Some("skills"),
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let mut state = SyncState::default();
        let report = sync_skills_one(
            spec,
            &master_skills,
            &mut state,
            Platform::MacOS,
            false,
            false,
        );

        match report.action {
            SkillsSyncAction::Updated {
                installed,
                forced: _,
            } => {
                assert_eq!(installed, vec!["caveman"])
            }
            other => panic!("expected Updated, got {:?}", other),
        }

        let harness_skill = harness_base.join("skills").join("caveman");
        assert!(harness_skill.exists() || harness_skill.is_symlink());
        assert!(skill_is_equivalent(
            &master_skills.join("caveman"),
            &harness_skill
        ));
    }

    #[test]
    fn sync_skills_detects_drift() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".pi").join("agent");
        fs::create_dir_all(&harness_base).unwrap();
        let harness_skills = harness_base.join("skills");
        fs::create_dir_all(&harness_skills).unwrap();
        fs::write(harness_skills.join("caveman"), "not a directory").unwrap();

        let spec = HarnessSpec {
            id: "pi",
            name: "Pi",
            mcp_format: McpFormat::JsonWithImports,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: Some("skills"),
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let mut state = SyncState::default();
        let report = sync_skills_one(
            spec,
            &master_skills,
            &mut state,
            Platform::MacOS,
            false,
            false,
        );

        assert_eq!(
            report.action,
            SkillsSyncAction::Drift {
                skills: vec!["caveman".into()]
            }
        );
    }

    #[test]
    fn sync_skills_preserves_harness_only_entries() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "shared", "SKILL.md", "x");

        let harness_base = tmp.path().join(".pi").join("agent");
        let harness_skills = harness_base.join("skills");
        fs::create_dir_all(&harness_skills).unwrap();
        fs::create_dir(harness_skills.join(".system")).unwrap();
        fs::write(harness_skills.join(".system").join("marker"), "sys").unwrap();

        let spec = HarnessSpec {
            id: "pi",
            name: "Pi",
            mcp_format: McpFormat::JsonWithImports,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: Some("skills"),
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let mut state = SyncState::default();
        sync_skills_one(
            spec,
            &master_skills,
            &mut state,
            Platform::MacOS,
            false,
            false,
        );

        assert!(harness_skills.join(".system").exists());
        assert!(
            harness_skills.join("shared").exists() || harness_skills.join("shared").is_symlink()
        );
    }

    #[test]
    fn sync_skills_force_overwrites_drift() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".pi").join("agent");
        fs::create_dir_all(&harness_base).unwrap();
        let harness_skills = harness_base.join("skills");
        fs::create_dir_all(&harness_skills).unwrap();
        fs::create_dir(harness_skills.join("caveman")).unwrap();
        fs::write(harness_skills.join("caveman").join("SKILL.md"), "old").unwrap();

        let spec = HarnessSpec {
            id: "pi",
            name: "Pi",
            mcp_format: McpFormat::JsonWithImports,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: Some("skills"),
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let mut state = SyncState::default();
        let report = sync_skills_one(
            spec,
            &master_skills,
            &mut state,
            Platform::MacOS,
            true,
            false,
        );

        match report.action {
            SkillsSyncAction::Updated {
                installed,
                forced: _,
            } => {
                assert_eq!(installed, vec!["caveman"])
            }
            other => panic!("expected Updated, got {:?}", other),
        }

        assert!(skill_is_equivalent(
            &master_skills.join("caveman"),
            &harness_skills.join("caveman")
        ));
    }

    #[test]
    fn sync_skills_already_in_sync() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".pi").join("agent");
        fs::create_dir_all(&harness_base).unwrap();
        let harness_skills = harness_base.join("skills");
        fs::create_dir_all(&harness_skills).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(
            master_skills.join("caveman"),
            harness_skills.join("caveman"),
        )
        .unwrap();
        #[cfg(not(unix))]
        copy_dir_all(
            &master_skills.join("caveman"),
            &harness_skills.join("caveman"),
        )
        .unwrap();

        let spec = HarnessSpec {
            id: "pi",
            name: "Pi",
            mcp_format: McpFormat::JsonWithImports,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: Some("skills"),
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let mut state = SyncState::default();
        let report = sync_skills_one(
            spec,
            &master_skills,
            &mut state,
            Platform::MacOS,
            false,
            false,
        );

        assert_eq!(report.action, SkillsSyncAction::AlreadyInSync);
    }

    #[test]
    fn sync_skills_no_skills_dir() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".cursor");
        fs::create_dir_all(&harness_base).unwrap();

        let spec = HarnessSpec {
            id: "cursor",
            name: "Cursor",
            mcp_format: McpFormat::Json,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: None,
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let mut state = SyncState::default();
        let report = sync_skills_one(
            spec,
            &master_skills,
            &mut state,
            Platform::MacOS,
            false,
            false,
        );

        assert_eq!(report.action, SkillsSyncAction::NoSkillsDir);
    }

    #[test]
    fn status_skills_in_sync() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".pi").join("agent");
        fs::create_dir_all(&harness_base).unwrap();
        let harness_skills = harness_base.join("skills");
        fs::create_dir_all(&harness_skills).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(
            master_skills.join("caveman"),
            harness_skills.join("caveman"),
        )
        .unwrap();
        #[cfg(not(unix))]
        copy_dir_all(
            &master_skills.join("caveman"),
            &harness_skills.join("caveman"),
        )
        .unwrap();

        let spec = HarnessSpec {
            id: "pi",
            name: "Pi",
            mcp_format: McpFormat::JsonWithImports,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: Some("skills"),
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let report = status_skills_one(spec, &master_skills, Platform::MacOS);
        assert_eq!(report.state, SkillsStatusState::InSync);
    }

    #[test]
    fn status_skills_missing() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".pi").join("agent");
        fs::create_dir_all(&harness_base).unwrap();

        let spec = HarnessSpec {
            id: "pi",
            name: "Pi",
            mcp_format: McpFormat::JsonWithImports,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: Some("skills"),
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let report = status_skills_one(spec, &master_skills, Platform::MacOS);
        assert_eq!(
            report.state,
            SkillsStatusState::Missing {
                skills: vec!["caveman".into()]
            }
        );
    }

    #[test]
    fn status_skills_drifted() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".pi").join("agent");
        fs::create_dir_all(&harness_base).unwrap();
        let harness_skills = harness_base.join("skills");
        fs::create_dir_all(&harness_skills).unwrap();
        fs::write(harness_skills.join("caveman"), "not a directory").unwrap();

        let spec = HarnessSpec {
            id: "pi",
            name: "Pi",
            mcp_format: McpFormat::JsonWithImports,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: Some("skills"),
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let report = status_skills_one(spec, &master_skills, Platform::MacOS);
        assert_eq!(
            report.state,
            SkillsStatusState::Drifted {
                skills: vec!["caveman".into()]
            }
        );
    }

    #[test]
    fn status_skills_no_skills_dir() {
        let tmp = TempDir::new().unwrap();
        let master_skills = tmp.path().join("skills");
        write_skill_file(&master_skills, "caveman", "SKILL.md", "grunt");

        let harness_base = tmp.path().join(".cursor");
        fs::create_dir_all(&harness_base).unwrap();

        let spec = HarnessSpec {
            id: "cursor",
            name: "Cursor",
            mcp_format: McpFormat::Json,
            macos_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            linux_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            windows_base: Box::leak(harness_base.to_string_lossy().to_string().into_boxed_str()),
            mcp_config_file: "mcp.json",
            mcp_config_absolute: None,
            skills_dir: None,
            agents_dir: None,
            detection_marker: "mcp.json",
        };
        let spec = Box::leak(Box::new(spec));

        let report = status_skills_one(spec, &master_skills, Platform::MacOS);
        assert_eq!(report.state, SkillsStatusState::NoSkillsDir);
    }

    #[test]
    fn import_skills_copies_from_source_to_target() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "caveman", "SKILL.md", "grunt");
        write_skill_file(&source, "diagnose", "SKILL.md", "debug");

        let target = tmp.path().join("Bridle").join("skills");

        let report = import_skills(&source, &target, false, false, false).unwrap();
        assert_eq!(report.imported, vec!["caveman", "diagnose"]);
        assert!(report.skipped.is_empty());
        assert!(report.errors.is_empty());

        assert!(target.join("caveman").is_dir());
        assert!(target.join("caveman").join("SKILL.md").exists());
        assert!(!target.join("caveman").is_symlink());
    }

    #[test]
    fn import_skills_skips_existing() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "caveman", "SKILL.md", "grunt");

        let target = tmp.path().join("Bridle").join("skills");
        write_skill_file(&target, "caveman", "SKILL.md", "existing");

        let report = import_skills(&source, &target, false, false, false).unwrap();
        assert!(report.imported.is_empty());
        assert_eq!(report.skipped, vec!["caveman"]);

        // Existing content should be preserved.
        let content = fs::read_to_string(target.join("caveman").join("SKILL.md")).unwrap();
        assert_eq!(content, "existing");
    }

    #[test]
    fn import_skills_force_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "caveman", "SKILL.md", "grunt");

        let target = tmp.path().join("Bridle").join("skills");
        write_skill_file(&target, "caveman", "SKILL.md", "existing");

        let report = import_skills(&source, &target, true, false, false).unwrap();
        assert_eq!(report.imported, vec!["caveman"]);
        assert!(report.skipped.is_empty());

        let content = fs::read_to_string(target.join("caveman").join("SKILL.md")).unwrap();
        assert_eq!(content, "grunt");
    }

    #[test]
    fn import_skills_follows_source_symlinks() {
        let tmp = TempDir::new().unwrap();
        let real_skill = tmp.path().join("real").join("caveman");
        write_skill_file(real_skill.parent().unwrap(), "caveman", "SKILL.md", "grunt");

        let source = tmp.path().join(".agents").join("skills");
        fs::create_dir_all(&source).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_skill, source.join("caveman")).unwrap();
        #[cfg(not(unix))]
        copy_dir_all(&real_skill, &source.join("caveman")).unwrap();

        let target = tmp.path().join("Bridle").join("skills");
        let report = import_skills(&source, &target, false, false, false).unwrap();
        assert_eq!(report.imported, vec!["caveman"]);

        assert!(target.join("caveman").is_dir());
        assert!(!target.join("caveman").is_symlink());
    }

    #[test]
    #[cfg(unix)]
    fn import_skills_link_creates_symlinks() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "caveman", "SKILL.md", "grunt");

        let target = tmp.path().join("Bridle").join("skills");
        let report = import_skills(&source, &target, false, true, false).unwrap();
        assert_eq!(report.imported, vec!["caveman"]);

        let target_skill = target.join("caveman");
        assert!(target_skill.is_symlink());
        assert_eq!(
            fs::read_link(&target_skill).unwrap(),
            fs::canonicalize(source.join("caveman")).unwrap()
        );
    }

    #[test]
    #[cfg(unix)]
    fn import_skills_link_skips_existing_correct_symlink() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "caveman", "SKILL.md", "grunt");

        let target = tmp.path().join("Bridle").join("skills");
        fs::create_dir_all(&target).unwrap();
        let source_canon = fs::canonicalize(source.join("caveman")).unwrap();
        std::os::unix::fs::symlink(&source_canon, target.join("caveman")).unwrap();

        let report = import_skills(&source, &target, false, true, false).unwrap();
        assert!(report.imported.is_empty());
        assert_eq!(report.skipped, vec!["caveman"]);
    }

    #[test]
    fn import_skills_update_reimports_changed_skills() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "caveman", "SKILL.md", "grunt");
        write_skill_file(&source, "diagnose", "SKILL.md", "debug");

        let target = tmp.path().join("Bridle").join("skills");
        // Copy initial versions into target.
        copy_dir_all(&source.join("caveman"), &target.join("caveman")).unwrap();
        copy_dir_all(&source.join("diagnose"), &target.join("diagnose")).unwrap();

        // Change only the source caveman skill.
        fs::write(source.join("caveman").join("SKILL.md"), "grunt v2").unwrap();

        let report = import_skills(&source, &target, false, false, true).unwrap();
        assert_eq!(report.imported, vec!["caveman"]);
        assert_eq!(report.skipped, vec!["diagnose"]);

        let content = fs::read_to_string(target.join("caveman").join("SKILL.md")).unwrap();
        assert_eq!(content, "grunt v2");
    }

    #[test]
    fn import_skills_update_skips_unchanged_skills() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "caveman", "SKILL.md", "grunt");

        let target = tmp.path().join("Bridle").join("skills");
        copy_dir_all(&source.join("caveman"), &target.join("caveman")).unwrap();

        let report = import_skills(&source, &target, false, false, true).unwrap();
        assert!(report.imported.is_empty());
        assert_eq!(report.skipped, vec!["caveman"]);
    }

    #[test]
    fn remove_skill_deletes_existing_skill() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        write_skill_file(&skills_dir, "caveman", "SKILL.md", "grunt");

        let removed = remove_skill(&skills_dir, "caveman").unwrap();
        assert!(removed);
        assert!(!skills_dir.join("caveman").exists());
    }

    #[test]
    fn remove_skill_returns_false_when_missing() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let removed = remove_skill(&skills_dir, "missing").unwrap();
        assert!(!removed);
    }

    #[test]
    fn remove_skill_can_remove_symlink() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();
        let real_skill = tmp.path().join("real");
        write_skill_file(&real_skill, "caveman", "SKILL.md", "grunt");

        #[cfg(unix)]
        std::os::unix::fs::symlink(real_skill.join("caveman"), skills_dir.join("caveman")).unwrap();
        #[cfg(not(unix))]
        copy_dir_all(&real_skill.join("caveman"), &skills_dir.join("caveman")).unwrap();

        let removed = remove_skill(&skills_dir, "caveman").unwrap();
        assert!(removed);
        assert!(!skills_dir.join("caveman").exists());
    }

    // ── Multi-source import tests ───────────────────────────────

    #[test]
    fn multi_source_import_no_collisions() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("Bridle").join("skills");

        let source_a = tmp.path().join(".project").join(".agents").join("skills");
        write_skill_file(&source_a, "only-in-a", "SKILL.md", "a");

        let source_b = tmp.path().join(".pi").join("agent").join("skills");
        write_skill_file(&source_b, "only-in-b", "SKILL.md", "b");

        let sources = vec![
            SkillSource {
                path: source_a,
                priority: SkillSourcePriority::Project,
            },
            SkillSource {
                path: source_b,
                priority: SkillSourcePriority::User,
            },
        ];

        let report = import_skills_from_sources(&sources, &target, false, false, false).unwrap();

        assert_eq!(report.imported, vec!["only-in-a", "only-in-b"]);
        assert!(report.skipped.is_empty());
        assert!(report.collisions.is_empty());
        assert!(report.errors.is_empty());

        assert!(target.join("only-in-a").is_dir());
        assert!(target.join("only-in-b").is_dir());
    }

    #[test]
    fn multi_source_project_wins_over_user_on_collision() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("Bridle").join("skills");

        let project = tmp.path().join(".project").join(".agents").join("skills");
        write_skill_file(&project, "shared", "SKILL.md", "project-version");

        let user_pi = tmp.path().join(".pi").join("agent").join("skills");
        write_skill_file(&user_pi, "shared", "SKILL.md", "user-version");

        let sources = vec![
            SkillSource {
                path: project.clone(),
                priority: SkillSourcePriority::Project,
            },
            SkillSource {
                path: user_pi.clone(),
                priority: SkillSourcePriority::User,
            },
        ];

        let report = import_skills_from_sources(&sources, &target, false, false, false).unwrap();

        assert_eq!(report.imported, vec!["shared"]);
        assert_eq!(report.collisions.len(), 1);

        let collision = &report.collisions[0];
        assert_eq!(collision.name, "shared");
        assert_eq!(collision.chosen.priority, SkillSourcePriority::Project);
        assert_eq!(collision.skipped.len(), 1);
        assert_eq!(collision.skipped[0].priority, SkillSourcePriority::User);

        // Project version should be the one installed.
        let content = fs::read_to_string(target.join("shared").join("SKILL.md")).unwrap();
        assert_eq!(content, "project-version");
    }

    #[test]
    fn multi_source_user_wins_over_default_on_collision() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("Bridle").join("skills");

        let user_pi = tmp.path().join(".pi").join("agent").join("skills");
        write_skill_file(&user_pi, "shared", "SKILL.md", "user-version");

        let default_agents = tmp.path().join(".agents").join("skills");
        write_skill_file(&default_agents, "shared", "SKILL.md", "default-version");

        let sources = vec![
            SkillSource {
                path: user_pi.clone(),
                priority: SkillSourcePriority::User,
            },
            SkillSource {
                path: default_agents.clone(),
                priority: SkillSourcePriority::Default,
            },
        ];

        let report = import_skills_from_sources(&sources, &target, false, false, false).unwrap();

        assert_eq!(report.imported, vec!["shared"]);
        assert_eq!(report.collisions.len(), 1);

        let collision = &report.collisions[0];
        assert_eq!(collision.name, "shared");
        assert_eq!(collision.chosen.priority, SkillSourcePriority::User);
        assert_eq!(collision.skipped.len(), 1);
        assert_eq!(collision.skipped[0].priority, SkillSourcePriority::Default);

        let content = fs::read_to_string(target.join("shared").join("SKILL.md")).unwrap();
        assert_eq!(content, "user-version");
    }

    #[test]
    fn multi_source_single_source_does_not_produce_collisions() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("Bridle").join("skills");

        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "alpha", "SKILL.md", "x");
        write_skill_file(&source, "beta", "SKILL.md", "y");

        let sources = vec![SkillSource {
            path: source,
            priority: SkillSourcePriority::User,
        }];

        let report = import_skills_from_sources(&sources, &target, false, false, false).unwrap();

        assert_eq!(report.imported, vec!["alpha", "beta"]);
        assert!(report.collisions.is_empty());
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn multi_source_force_overwrites_target() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("Bridle").join("skills");

        let project = tmp.path().join(".project").join(".agents").join("skills");
        write_skill_file(&project, "caveman", "SKILL.md", "project");

        let user_pi = tmp.path().join(".pi").join("agent").join("skills");
        write_skill_file(&user_pi, "caveman", "SKILL.md", "user");

        // Pre-populate target with a different version.
        write_skill_file(&target, "caveman", "SKILL.md", "old");

        let sources = vec![
            SkillSource {
                path: project.clone(),
                priority: SkillSourcePriority::Project,
            },
            SkillSource {
                path: user_pi.clone(),
                priority: SkillSourcePriority::User,
            },
        ];

        let report = import_skills_from_sources(&sources, &target, true, false, false).unwrap();

        assert_eq!(report.imported, vec!["caveman"]);
        let content = fs::read_to_string(target.join("caveman").join("SKILL.md")).unwrap();
        assert_eq!(content, "project");
    }

    #[test]
    fn multi_source_skips_unchanged_target() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("Bridle").join("skills");

        let source = tmp.path().join(".agents").join("skills");
        write_skill_file(&source, "caveman", "SKILL.md", "grunt");

        // Pre-populate target with identical content.
        copy_dir_all(&source.join("caveman"), &target.join("caveman")).unwrap();

        let sources = vec![SkillSource {
            path: source,
            priority: SkillSourcePriority::User,
        }];

        let report = import_skills_from_sources(&sources, &target, false, false, false).unwrap();

        assert!(report.imported.is_empty());
        assert_eq!(report.skipped, vec!["caveman"]);
    }

    #[test]
    fn discover_skill_sources_finds_project_source() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("my-project");
        let agents_skills = project.join(".agents").join("skills");
        fs::create_dir_all(&agents_skills).unwrap();

        let cwd = project.join("src").join("deep");
        fs::create_dir_all(&cwd).unwrap();

        let sources = discover_skill_sources(Platform::MacOS, &cwd);

        let project_sources: Vec<_> = sources
            .iter()
            .filter(|s| s.priority == SkillSourcePriority::Project)
            .collect();
        assert_eq!(project_sources.len(), 1);
        // resolved paths may differ, but the relative structure matches
        assert!(project_sources[0].path.ends_with(".agents/skills"));
    }
}
