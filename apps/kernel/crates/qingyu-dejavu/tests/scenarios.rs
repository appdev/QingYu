// Derived from siyuan-note/dejavu test/sync/sync_scenario_test.go at
// 8462fe30163c6e6e95ae2da832cfe76058e0e830 (AGPL-3.0-only).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use filetime::FileTime;
use qingyu_dejavu::{
    derive_key, Cloud, Device, LocalCloud, MergeResult, NoopWorkingTreeCoordinator, Repo,
    RepoOptions, RepoPaths, WorkingTreeCoordinator,
};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const BASIC: &str = include_str!("fixtures/dejavu/cases/basic/config.json");
const EDGE: &str = include_str!("fixtures/dejavu/cases/edge/config.json");
const KNOWN_CONFLICTS: &str = include_str!("fixtures/dejavu/cases/known-conflicts/config.json");
const SYNC_DOWNLOAD: &str = include_str!("fixtures/dejavu/cases/sync-download/config.json");
const BASE_TIME_SECONDS: i64 = 1_700_000_000;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioCase {
    name: String,
    #[serde(default)]
    skip: String,
    #[serde(default)]
    seed: BTreeMap<String, String>,
    #[serde(default, rename = "seedDir")]
    seed_dir: String,
    clients: Vec<String>,
    steps: Vec<ScenarioStep>,
    #[serde(default, rename = "final")]
    final_state: BTreeMap<String, ScenarioClientState>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "op")]
enum ScenarioStep {
    #[serde(rename = "write")]
    Write(WriteStep),
    #[serde(rename = "apply_dir")]
    ApplyDir(ApplyDirStep),
    #[serde(rename = "remove")]
    Remove(RemoveStep),
    #[serde(rename = "index")]
    Index(IndexStep),
    #[serde(rename = "sync")]
    Sync(SyncStep),
    #[serde(rename = "sync_download")]
    SyncDownload(SyncStep),
    #[serde(rename = "assert")]
    Assert(AssertStep),
    #[serde(rename = "assert_missing")]
    AssertMissing(AssertMissingStep),
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteStep {
    client: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    minutes: i64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyDirStep {
    client: String,
    #[serde(default, rename = "sourceDir")]
    source_dir: String,
    #[serde(default)]
    minutes: i64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveStep {
    client: String,
    #[serde(default)]
    path: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexStep {
    client: String,
    #[serde(default)]
    memo: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncStep {
    client: String,
    #[serde(default)]
    want: Option<ScenarioExpectation>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssertStep {
    client: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssertMissingStep {
    client: String,
    #[serde(default)]
    path: String,
}

impl ScenarioStep {
    fn client(&self) -> &str {
        match self {
            Self::Write(step) => &step.client,
            Self::ApplyDir(step) => &step.client,
            Self::Remove(step) => &step.client,
            Self::Index(step) => &step.client,
            Self::Sync(step) | Self::SyncDownload(step) => &step.client,
            Self::Assert(step) => &step.client,
            Self::AssertMissing(step) => &step.client,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Write(_) => "write",
            Self::ApplyDir(_) => "apply_dir",
            Self::Remove(_) => "remove",
            Self::Index(_) => "index",
            Self::Sync(_) => "sync",
            Self::SyncDownload(_) => "sync_download",
            Self::Assert(_) => "assert",
            Self::AssertMissing(_) => "assert_missing",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioExpectation {
    #[serde(default)]
    upserts: usize,
    #[serde(default)]
    removes: usize,
    #[serde(default)]
    conflicts: usize,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioClientState {
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    sources: BTreeMap<String, String>,
    #[serde(default)]
    missing: Vec<String>,
}

struct LoadedCase {
    fixture: Fixture,
    case: ScenarioCase,
}

#[derive(Clone, Copy)]
struct Fixture {
    group: &'static str,
    upstream_path: &'static str,
    sha256: &'static str,
}

struct ApprovedDeviation {
    fixture_path: &'static str,
    fixture_sha256: &'static str,
    case: &'static str,
    step: Option<usize>,
    op: &'static str,
    client: &'static str,
    path: &'static str,
    upstream: Option<&'static [u8]>,
    qingyu: Option<&'static [u8]>,
}

const APPROVED_DEVIATIONS: &[ApprovedDeviation] = &[
    ApprovedDeviation {
        fixture_path: "test/sync/testdata/cases/known-conflicts/config.json",
        fixture_sha256: "40941ce32657ff4fe08379a61e8e4e3ff2bf2ed7f489f46654b758cfb51b8596",
        case: "sync download structured content merge candidate reports conflict",
        step: None,
        op: "final",
        client: "b",
        path: "doc.txt",
        upstream: Some(b"first\n\nsecond\n"),
        qingyu: Some(b"first\nsecond changed\n"),
    },
    ApprovedDeviation {
        fixture_path: "test/sync/testdata/cases/sync-download/config.json",
        fixture_sha256: "6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24",
        case: "sync download remote update conflicts with independent local edit",
        step: Some(8),
        op: "assert",
        client: "b",
        path: "local.txt",
        upstream: Some(b"local base\n"),
        qingyu: Some(b"local changed\n"),
    },
    ApprovedDeviation {
        fixture_path: "test/sync/testdata/cases/sync-download/config.json",
        fixture_sha256: "6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24",
        case: "sync download remote delete conflicts with independent local edit",
        step: Some(8),
        op: "assert",
        client: "b",
        path: "local.txt",
        upstream: Some(b"local base\n"),
        qingyu: Some(b"local changed\n"),
    },
    ApprovedDeviation {
        fixture_path: "test/sync/testdata/cases/sync-download/config.json",
        fixture_sha256: "6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24",
        case: "sync download remote update conflicts with local update",
        step: Some(7),
        op: "assert",
        client: "b",
        path: "doc.txt",
        upstream: Some(b"from a\n"),
        qingyu: Some(b"from b\n"),
    },
    ApprovedDeviation {
        fixture_path: "test/sync/testdata/cases/sync-download/config.json",
        fixture_sha256: "6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24",
        case: "sync download remote delete conflicts with local update",
        step: Some(7),
        op: "assert_missing",
        client: "b",
        path: "doc.txt",
        upstream: None,
        qingyu: Some(b"from b\n"),
    },
    ApprovedDeviation {
        fixture_path: "test/sync/testdata/cases/sync-download/config.json",
        fixture_sha256: "6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24",
        case: "sync download remote update restores over local delete",
        step: Some(7),
        op: "assert",
        client: "b",
        path: "doc.txt",
        upstream: Some(b"from a\n"),
        qingyu: None,
    },
    ApprovedDeviation {
        fixture_path: "test/sync/testdata/cases/sync-download/config.json",
        fixture_sha256: "6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24",
        case: "sync download remote create conflicts with local create at same path",
        step: Some(7),
        op: "assert",
        client: "b",
        path: "new.txt",
        upstream: Some(b"from a\n"),
        qingyu: Some(b"from b\n"),
    },
];

struct ApprovedDeviationTracker {
    consumed: Vec<bool>,
}

struct FilesystemMismatch<'a> {
    fixture: Fixture,
    case: &'a str,
    step: Option<usize>,
    op: &'a str,
    client: &'a str,
    relative: &'a str,
    upstream_expected: Option<&'a [u8]>,
    actual: Option<&'a [u8]>,
}

impl ApprovedDeviationTracker {
    fn new() -> Self {
        Self {
            consumed: vec![false; APPROVED_DEVIATIONS.len()],
        }
    }

    fn accept_filesystem_mismatch(
        &mut self,
        mismatch: FilesystemMismatch<'_>,
    ) -> Result<(), String> {
        let FilesystemMismatch {
            fixture,
            case,
            step,
            op,
            client,
            relative,
            upstream_expected,
            actual,
        } = mismatch;
        let Some((index, deviation)) = APPROVED_DEVIATIONS.iter().enumerate().find(|(_, item)| {
            item.fixture_path == fixture.upstream_path
                && item.fixture_sha256 == fixture.sha256
                && item.case == case
                && item.step == step
                && item.op == op
                && item.client == client
                && item.path == relative
        }) else {
            return Err(format!(
                "unapproved filesystem mismatch: fixture={} sha256={} case={case:?} location={} op={op:?} client={client:?} path={relative:?}, upstream={}, actual={}",
                fixture.upstream_path,
                fixture.sha256,
                deviation_location(step),
                display_file_state(upstream_expected),
                display_file_state(actual)
            ));
        };
        if !same_file_state(upstream_expected, deviation.upstream) {
            return Err(format!(
                "approved deviation upstream expectation changed: fixture={} case={case:?} location={} op={op:?} client={client:?} path={relative:?}, pinned={}, fixture={}",
                fixture.upstream_path,
                deviation_location(step),
                display_file_state(deviation.upstream),
                display_file_state(upstream_expected)
            ));
        }
        if !same_file_state(actual, deviation.qingyu) {
            return Err(format!(
                "approved deviation QingYu result changed: fixture={} case={case:?} location={} op={op:?} client={client:?} path={relative:?}, expected={}, actual={}",
                fixture.upstream_path,
                deviation_location(step),
                display_file_state(deviation.qingyu),
                display_file_state(actual)
            ));
        }
        if self.consumed[index] {
            return Err(format!(
                "approved deviation consumed more than once: fixture={} case={case:?} location={} op={op:?} client={client:?} path={relative:?}",
                fixture.upstream_path,
                deviation_location(step)
            ));
        }
        self.consumed[index] = true;
        Ok(())
    }

    fn assert_all_consumed(&self) -> Result<(), String> {
        let unused = APPROVED_DEVIATIONS
            .iter()
            .zip(&self.consumed)
            .filter(|(_, consumed)| !**consumed)
            .map(|(item, _)| {
                format!(
                    "{} sha256={} case={:?} location={} op={:?} client={:?} path={:?}",
                    item.fixture_path,
                    item.fixture_sha256,
                    item.case,
                    deviation_location(item.step),
                    item.op,
                    item.client,
                    item.path
                )
            })
            .collect::<Vec<_>>();
        if unused.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "approved final filesystem deviations were not consumed: {}",
                unused.join(", ")
            ))
        }
    }
}

fn same_file_state(left: Option<&[u8]>, right: Option<&[u8]>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        (None, None) => true,
        _ => false,
    }
}

fn display_file_state(state: Option<&[u8]>) -> String {
    state.map_or_else(
        || "missing".to_owned(),
        |bytes| format!("bytes({:?})", String::from_utf8_lossy(bytes)),
    )
}

fn deviation_location(step: Option<usize>) -> String {
    step.map_or_else(|| "final".to_owned(), |number| format!("step {number}"))
}

#[derive(Clone)]
struct ClientPaths {
    data: PathBuf,
    repo: PathBuf,
    history: PathBuf,
    temp: PathBuf,
}

struct ScenarioClient {
    name: String,
    paths: ClientPaths,
    repo: Repo,
}

struct ScenarioEnv {
    _root: TempDir,
    fixture_dir: PathBuf,
    clients_root: PathBuf,
    cloud: Arc<LocalCloud>,
    key: [u8; 32],
}

fn load_cases() -> Vec<LoadedCase> {
    let fixtures = [
        (
            Fixture {
                group: "basic",
                upstream_path: "test/sync/testdata/cases/basic/config.json",
                sha256: "1b4c0ef8c3c39e0b971260f30ff9bad4120f56d23d00cab42d78be97a1693268",
            },
            BASIC,
            7_usize,
        ),
        (
            Fixture {
                group: "edge",
                upstream_path: "test/sync/testdata/cases/edge/config.json",
                sha256: "eef8aa5389688989a39da511c30bbad87501ea04dee2fcf8b67ae288f5df1875",
            },
            EDGE,
            5,
        ),
        (
            Fixture {
                group: "known-conflicts",
                upstream_path: "test/sync/testdata/cases/known-conflicts/config.json",
                sha256: "40941ce32657ff4fe08379a61e8e4e3ff2bf2ed7f489f46654b758cfb51b8596",
            },
            KNOWN_CONFLICTS,
            4,
        ),
        (
            Fixture {
                group: "sync-download",
                upstream_path: "test/sync/testdata/cases/sync-download/config.json",
                sha256: "6134a21d9deee1381498beb899a8ed2667c6c98f4b08e91f217d3cfff89fec24",
            },
            SYNC_DOWNLOAD,
            11,
        ),
    ];
    let mut cases = Vec::new();
    for (fixture, json, expected) in fixtures {
        let actual_sha256 = format!("{:x}", Sha256::digest(json.as_bytes()));
        assert_eq!(
            actual_sha256, fixture.sha256,
            "fixture hash changed for {}",
            fixture.upstream_path
        );
        let fixture_cases = serde_json::from_str::<Vec<ScenarioCase>>(json)
            .unwrap_or_else(|error| panic!("parse {} scenarios: {error}", fixture.group));
        assert_eq!(
            fixture_cases.len(),
            expected,
            "unexpected scenario count for {}",
            fixture.group
        );
        cases.extend(
            fixture_cases
                .into_iter()
                .map(|case| LoadedCase { fixture, case }),
        );
    }
    assert_eq!(cases.len(), 7 + 5 + 4 + 11);
    cases
}

impl ScenarioEnv {
    async fn new(fixture_group: &str) -> Result<Self, String> {
        let root = tempfile::tempdir().map_err(|error| format!("create scenario root: {error}"))?;
        let cloud_root = root.path().join("cloud");
        let clients_root = root.path().join("clients");
        fs::create_dir_all(&cloud_root)
            .map_err(|error| format!("create cloud root {}: {error}", cloud_root.display()))?;
        fs::create_dir_all(&clients_root)
            .map_err(|error| format!("create clients root {}: {error}", clients_root.display()))?;
        let cloud = Arc::new(
            LocalCloud::new(&cloud_root)
                .map_err(|error| format!("open local cloud {}: {error}", cloud_root.display()))?,
        );
        let key = derive_key("pass", "salt").map_err(|error| format!("derive key: {error}"))?;
        let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/dejavu/cases")
            .join(fixture_group);

        Ok(Self {
            _root: root,
            fixture_dir,
            clients_root,
            cloud,
            key,
        })
    }

    async fn clients_for_case(
        &self,
        case: &ScenarioCase,
    ) -> Result<BTreeMap<String, ScenarioClient>, String> {
        if case.seed.is_empty() && case.seed_dir.is_empty() {
            return Err(format!(
                "case {}: seed must contain at least one file or seedDir",
                case.name
            ));
        }

        let seed_paths = self.empty_client_paths("seed")?;
        if !case.seed_dir.is_empty() {
            let source = safe_join(&self.fixture_dir, &case.seed_dir, "fixture")?;
            copy_dir_into(&source, &seed_paths.data)?;
            touch_regular_files(&seed_paths.data, scenario_time(0)?)?;
        }
        for (path, content) in &case.seed {
            write_file_exact(
                &seed_paths.data,
                path,
                content.as_bytes(),
                scenario_time(0)?,
            )?;
        }

        let seed_repo = self.open_repo("seed", &seed_paths)?;
        seed_repo
            .index("seed")
            .map_err(|error| format!("case {} seed index: {error}", case.name))?;
        let cloud: Arc<dyn Cloud> = self.cloud.clone();
        let coordinator: Arc<dyn WorkingTreeCoordinator> = Arc::new(NoopWorkingTreeCoordinator);
        let (seed_merge, _) = seed_repo
            .sync(cloud, coordinator)
            .await
            .map_err(|error| format!("case {} seed sync: {error}", case.name))?;
        assert_merge(
            &case.name,
            "seed sync",
            "seed",
            &seed_merge,
            ScenarioExpectation::default(),
        )?;
        drop(seed_repo);

        let mut clients = BTreeMap::new();
        for name in &case.clients {
            if name.is_empty() {
                return Err(format!("case {}: empty client name", case.name));
            }
            if clients.contains_key(name) {
                return Err(format!("case {}: duplicate client {name}", case.name));
            }
            let paths = self.empty_client_paths(name)?;
            copy_dir_into(&seed_paths.data, &paths.data)?;
            copy_dir_into(&seed_paths.repo, &paths.repo)?;
            let repo = self.open_repo(name, &paths)?;
            clients.insert(
                name.clone(),
                ScenarioClient {
                    name: name.clone(),
                    paths,
                    repo,
                },
            );
        }
        Ok(clients)
    }

    fn empty_client_paths(&self, name: &str) -> Result<ClientPaths, String> {
        let root = safe_join(&self.clients_root, name, "client")?;
        let paths = ClientPaths {
            data: root.join("data"),
            repo: root.join("repo"),
            history: root.join("history"),
            temp: root.join("temp"),
        };
        for directory in [&paths.data, &paths.repo, &paths.history, &paths.temp] {
            fs::create_dir_all(directory).map_err(|error| {
                format!("create client directory {}: {error}", directory.display())
            })?;
        }
        Ok(paths)
    }

    fn open_repo(&self, name: &str, paths: &ClientPaths) -> Result<Repo, String> {
        Repo::open(
            RepoPaths {
                data: paths.data.clone(),
                repo: paths.repo.clone(),
                history: paths.history.clone(),
                temp: paths.temp.clone(),
            },
            Device {
                id: name.to_owned(),
                name: name.to_owned(),
                os: std::env::consts::OS.to_owned(),
            },
            self.key,
            RepoOptions::default(),
        )
        .map_err(|error| format!("open repo for client {name}: {error}"))
    }
}

async fn run_case(
    loaded: &LoadedCase,
    deviations: &mut ApprovedDeviationTracker,
) -> Result<(), String> {
    let case = &loaded.case;
    if !case.skip.is_empty() {
        return Ok(());
    }

    let env = ScenarioEnv::new(loaded.fixture.group).await?;
    let clients = env.clients_for_case(case).await?;
    for (index, step) in case.steps.iter().enumerate() {
        let client = clients.get(step.client()).ok_or_else(|| {
            format!(
                "case {} step {}: unknown client {}",
                case.name,
                index + 1,
                step.client()
            )
        })?;
        run_step(
            &env,
            loaded.fixture,
            &case.name,
            index + 1,
            client,
            step,
            deviations,
        )
        .await?;
    }
    assert_final(&env, loaded.fixture, case, &clients, deviations)
}

async fn run_step(
    env: &ScenarioEnv,
    fixture: Fixture,
    case_name: &str,
    step_number: usize,
    client: &ScenarioClient,
    step: &ScenarioStep,
    deviations: &mut ApprovedDeviationTracker,
) -> Result<(), String> {
    let step_label = format!(
        "case {case_name} step {step_number} op {}",
        step.operation()
    );
    match step {
        ScenarioStep::Write(step) => {
            let content = if step.source.is_empty() {
                step.content.as_bytes().to_vec()
            } else {
                let source = safe_join(&env.fixture_dir, &step.source, "fixture")?;
                fs::read(&source)
                    .map_err(|error| format!("{step_label}: read {}: {error}", source.display()))?
            };
            write_file_exact(
                &client.paths.data,
                &step.path,
                &content,
                scenario_time(step.minutes)?,
            )
            .map_err(|error| format!("{step_label}: {error}"))
        }
        ScenarioStep::ApplyDir(step) => {
            if step.source_dir.is_empty() {
                return Err(format!("{step_label}: empty sourceDir"));
            }
            let source = safe_join(&env.fixture_dir, &step.source_dir, "fixture")?;
            copy_dir_into(&source, &client.paths.data)
                .map_err(|error| format!("{step_label}: {error}"))?;
            touch_regular_files(&client.paths.data, scenario_time(step.minutes)?)
                .map_err(|error| format!("{step_label}: {error}"))
        }
        ScenarioStep::Remove(step) => {
            let path = safe_join(&client.paths.data, &step.path, "data")?;
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!("{step_label}: remove {}: {error}", path.display())),
            }
        }
        ScenarioStep::Index(step) => {
            let memo = if step.memo.is_empty() {
                "sync scenario index"
            } else {
                &step.memo
            };
            client
                .repo
                .index(memo)
                .map(|_| ())
                .map_err(|error| format!("{step_label}: index {}: {error}", client.name))
        }
        ScenarioStep::Sync(step) => {
            let cloud: Arc<dyn Cloud> = env.cloud.clone();
            let coordinator: Arc<dyn WorkingTreeCoordinator> = Arc::new(NoopWorkingTreeCoordinator);
            let result = client
                .repo
                .sync(cloud, coordinator)
                .await
                .map_err(|error| format!("{step_label}: client {}: {error}", client.name))?;
            if let Some(expected) = step.want {
                assert_merge(case_name, &step_label, &client.name, &result.0, expected)?;
            }
            Ok(())
        }
        ScenarioStep::SyncDownload(step) => {
            let cloud: Arc<dyn Cloud> = env.cloud.clone();
            let coordinator: Arc<dyn WorkingTreeCoordinator> = Arc::new(NoopWorkingTreeCoordinator);
            let result = client
                .repo
                .sync_download(cloud, coordinator)
                .await
                .map_err(|error| format!("{step_label}: client {}: {error}", client.name))?;
            if let Some(expected) = step.want {
                assert_merge(case_name, &step_label, &client.name, &result.0, expected)?;
            }
            Ok(())
        }
        ScenarioStep::Assert(step) => {
            let actual = read_file_state(&client.paths.data, &step.path)
                .map_err(|error| format!("{step_label}: client {}: {error}", client.name))?;
            if same_file_state(actual.as_deref(), Some(step.content.as_bytes())) {
                Ok(())
            } else {
                deviations.accept_filesystem_mismatch(FilesystemMismatch {
                    fixture,
                    case: case_name,
                    step: Some(step_number),
                    op: "assert",
                    client: &client.name,
                    relative: &step.path,
                    upstream_expected: Some(step.content.as_bytes()),
                    actual: actual.as_deref(),
                })
            }
        }
        ScenarioStep::AssertMissing(step) => {
            let actual = read_file_state(&client.paths.data, &step.path)
                .map_err(|error| format!("{step_label}: client {}: {error}", client.name))?;
            if actual.is_none() {
                Ok(())
            } else {
                deviations.accept_filesystem_mismatch(FilesystemMismatch {
                    fixture,
                    case: case_name,
                    step: Some(step_number),
                    op: "assert_missing",
                    client: &client.name,
                    relative: &step.path,
                    upstream_expected: None,
                    actual: actual.as_deref(),
                })
            }
        }
    }
}

fn assert_merge(
    case_name: &str,
    operation: &str,
    client: &str,
    actual: &MergeResult,
    expected: ScenarioExpectation,
) -> Result<(), String> {
    let actual_counts = (
        actual.upserts.len(),
        actual.removes.len(),
        actual.conflicts.len(),
    );
    let expected_counts = (expected.upserts, expected.removes, expected.conflicts);
    if actual_counts == expected_counts {
        return Ok(());
    }
    Err(format!(
        "case {case_name} {operation}: client {client} merge mismatch: expected upserts={} removes={} conflicts={}, actual upserts={} removes={} conflicts={}",
        expected_counts.0,
        expected_counts.1,
        expected_counts.2,
        actual_counts.0,
        actual_counts.1,
        actual_counts.2
    ))
}

fn assert_final(
    env: &ScenarioEnv,
    fixture: Fixture,
    case: &ScenarioCase,
    clients: &BTreeMap<String, ScenarioClient>,
    deviations: &mut ApprovedDeviationTracker,
) -> Result<(), String> {
    for (client_name, state) in &case.final_state {
        let client = clients
            .get(client_name)
            .ok_or_else(|| format!("case {} final: unknown client {client_name}", case.name))?;
        for (path, content) in &state.files {
            let actual = read_file_state(&client.paths.data, path).map_err(|error| {
                format!("case {} final client {client_name}: {error}", case.name)
            })?;
            if !same_file_state(actual.as_deref(), Some(content.as_bytes())) {
                deviations.accept_filesystem_mismatch(FilesystemMismatch {
                    fixture,
                    case: &case.name,
                    step: None,
                    op: "final",
                    client: client_name,
                    relative: path,
                    upstream_expected: Some(content.as_bytes()),
                    actual: actual.as_deref(),
                })?;
            }
        }
        for (path, source) in &state.sources {
            let source_path = safe_join(&env.fixture_dir, source, "fixture")?;
            let expected = fs::read(&source_path).map_err(|error| {
                format!(
                    "case {} final: read {}: {error}",
                    case.name,
                    source_path.display()
                )
            })?;
            let actual = read_file_state(&client.paths.data, path).map_err(|error| {
                format!("case {} final client {client_name}: {error}", case.name)
            })?;
            if !same_file_state(actual.as_deref(), Some(&expected)) {
                deviations.accept_filesystem_mismatch(FilesystemMismatch {
                    fixture,
                    case: &case.name,
                    step: None,
                    op: "final",
                    client: client_name,
                    relative: path,
                    upstream_expected: Some(&expected),
                    actual: actual.as_deref(),
                })?;
            }
        }
        for path in &state.missing {
            let actual = read_file_state(&client.paths.data, path).map_err(|error| {
                format!("case {} final client {client_name}: {error}", case.name)
            })?;
            if actual.is_some() {
                deviations.accept_filesystem_mismatch(FilesystemMismatch {
                    fixture,
                    case: &case.name,
                    step: None,
                    op: "final",
                    client: client_name,
                    relative: path,
                    upstream_expected: None,
                    actual: actual.as_deref(),
                })?;
            }
        }
    }
    Ok(())
}

fn scenario_time(minutes: i64) -> Result<FileTime, String> {
    let offset = minutes
        .checked_mul(60)
        .ok_or_else(|| format!("scenario minutes overflow: {minutes}"))?;
    let seconds = BASE_TIME_SECONDS
        .checked_add(offset)
        .ok_or_else(|| format!("scenario timestamp overflow: {minutes}"))?;
    Ok(FileTime::from_unix_time(seconds, 0))
}

fn safe_join(base: &Path, relative: &str, kind: &str) -> Result<PathBuf, String> {
    if relative.is_empty() || relative.contains('\\') {
        return Err(format!("invalid {kind} path {relative:?}"));
    }
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("invalid {kind} path {relative:?}"));
    }
    Ok(base.join(path))
}

fn write_file_exact(
    data_root: &Path,
    relative: &str,
    content: &[u8],
    timestamp: FileTime,
) -> Result<(), String> {
    let path = safe_join(data_root, relative, "data")?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("file has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("create parent {}: {error}", parent.display()))?;
    fs::write(&path, content).map_err(|error| format!("write {}: {error}", path.display()))?;
    filetime::set_file_times(&path, timestamp, timestamp)
        .map_err(|error| format!("set timestamp {}: {error}", path.display()))?;
    assert_mtime(&path, timestamp)
}

fn assert_mtime(path: &Path, expected: FileTime) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("stat timestamp {}: {error}", path.display()))?;
    let actual = FileTime::from_last_modification_time(&metadata);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "timestamp mismatch for {}: expected {}.{:09}, actual {}.{:09}",
            path.display(),
            expected.unix_seconds(),
            expected.nanoseconds(),
            actual.unix_seconds(),
            actual.nanoseconds()
        ))
    }
}

fn copy_dir_into(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("stat source directory {}: {error}", source.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "source is not a safe directory: {}",
            source.display()
        ));
    }
    fs::create_dir_all(destination)
        .map_err(|error| format!("create directory {}: {error}", destination.display()))?;
    copy_dir_contents(source, destination)
}

fn copy_dir_contents(source: &Path, destination: &Path) -> Result<(), String> {
    let mut entries = fs::read_dir(source)
        .map_err(|error| format!("read directory {}: {error}", source.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read directory entry {}: {error}", source.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("read file type {}: {error}", source_path.display()))?;
        if file_type.is_symlink() {
            return Err(format!("refuse to copy symlink {}", source_path.display()));
        }
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).map_err(|error| {
                format!("create directory {}: {error}", destination_path.display())
            })?;
            copy_dir_contents(&source_path, &destination_path)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(format!(
                "refuse to copy special file {}",
                source_path.display()
            ));
        }
        copy_regular_file(&source_path, &destination_path)?;
    }
    Ok(())
}

fn copy_regular_file(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata =
        fs::metadata(source).map_err(|error| format!("stat file {}: {error}", source.display()))?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create directory {}: {error}", parent.display()))?;
    }
    fs::copy(source, destination).map_err(|error| {
        format!(
            "copy file {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    fs::set_permissions(destination, metadata.permissions())
        .map_err(|error| format!("set permissions {}: {error}", destination.display()))?;
    let mtime = FileTime::from_last_modification_time(&metadata);
    let atime = FileTime::from_last_access_time(&metadata);
    filetime::set_file_times(destination, atime, mtime)
        .map_err(|error| format!("set timestamp {}: {error}", destination.display()))?;
    assert_mtime(destination, mtime)
}

fn touch_regular_files(directory: &Path, timestamp: FileTime) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("read directory {}: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read directory entry {}: {error}", directory.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("read file type {}: {error}", path.display()))?;
        if file_type.is_symlink() {
            return Err(format!("refuse to touch symlink {}", path.display()));
        }
        if file_type.is_dir() {
            touch_regular_files(&path, timestamp)?;
        } else if file_type.is_file() {
            filetime::set_file_times(&path, timestamp, timestamp)
                .map_err(|error| format!("set timestamp {}: {error}", path.display()))?;
            assert_mtime(&path, timestamp)?;
        } else {
            return Err(format!("refuse to touch special file {}", path.display()));
        }
    }
    Ok(())
}

fn read_file_state(data_root: &Path, relative: &str) -> Result<Option<Vec<u8>>, String> {
    let path = safe_join(data_root, relative, "data")?;
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

fn assert_scenario_step_parse_error(json: &str, expected_message: &str) {
    let error =
        serde_json::from_str::<ScenarioStep>(json).expect_err("scenario step should be rejected");
    assert!(
        error.to_string().contains(expected_message),
        "unexpected scenario parse error: {error}"
    );
}

#[test]
fn sync_step_rejects_path_field() {
    assert_scenario_step_parse_error(
        r#"{"op":"sync","client":"a","path":"doc.txt"}"#,
        "unknown field `path`",
    );
}

#[test]
fn remove_step_rejects_want_field() {
    assert_scenario_step_parse_error(
        r#"{"op":"remove","client":"a","path":"doc.txt","want":{"upserts":0}}"#,
        "unknown field `want`",
    );
}

#[test]
fn scenario_step_rejects_unknown_operation() {
    assert_scenario_step_parse_error(
        r#"{"op":"teleport","client":"a"}"#,
        "unknown variant `teleport`",
    );
}

#[tokio::test]
async fn runs_all_pinned_dejavu_scenarios() {
    let mut deviations = ApprovedDeviationTracker::new();
    for loaded in load_cases() {
        run_case(&loaded, &mut deviations)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }
    deviations
        .assert_all_consumed()
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn approved_filesystem_deviation_allowlist_has_exact_size() {
    assert_eq!(APPROVED_DEVIATIONS.len(), 7);
}
