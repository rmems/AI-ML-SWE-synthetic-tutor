//! Local, non-release smoke for the derivative-misconceptions pilot (#20).
//! Exercises the user-facing generator and student simulator against the real
//! Julia worker, then validates a five-row dialogue fixture in memory.

use std::collections::BTreeSet;
use std::fs;
use std::net::SocketAddrV4;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tutor_schema::{
    Answer, DIALOGUE_SCHEMA_VERSION, DialogueProvenance, DialogueRow, MisconceptionId,
    NamedVersion, Persistence, Problem, Role, RowSource, RowType, StudentAttempt, TAXONOMY_VERSION,
    TUTOR_POLICY_VERSION, TeacherVarietyStatus, Turn, TurnKind,
};
use tutor_verify::{JULIA_RECOMPUTE, RUST_RECOMPUTE};

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/tutor-system-v1.txt");

struct FixtureCase {
    generator: &'static str,
    seed: &'static str,
    misconception: Option<MisconceptionId>,
    persistence: Persistence,
}

const CASES: [FixtureCase; 5] = [
    FixtureCase {
        generator: "product_rule_at_point",
        seed: "1",
        misconception: Some(MisconceptionId::DerivativesProductRule),
        persistence: Persistence::CorrectsAfterL1,
    },
    FixtureCase {
        generator: "product_rule_at_point",
        seed: "2",
        misconception: Some(MisconceptionId::DerivativesProductRule),
        persistence: Persistence::CorrectsAfterL2,
    },
    FixtureCase {
        generator: "chain_rule_at_point",
        seed: "3",
        misconception: Some(MisconceptionId::DerivativesChainRule),
        persistence: Persistence::CorrectsAfterL1,
    },
    FixtureCase {
        generator: "chain_rule_at_point",
        seed: "4",
        misconception: Some(MisconceptionId::DerivativesChainRule),
        persistence: Persistence::CorrectsAfterL2,
    },
    FixtureCase {
        generator: "binomial_derivative_at_point",
        seed: "1",
        misconception: None,
        persistence: Persistence::CorrectsAfterL1,
    },
];

#[test]
#[ignore = "requires a running local TutorMath Julia worker"]
fn derivative_pilot_replays_verified_procedural_dialogues() {
    let scratch = Scratch::new();
    let endpoint = local_endpoint();
    let first = build_fixture(&scratch.0.join("first"), &endpoint);
    let replay = build_fixture(&scratch.0.join("replay"), &endpoint);
    assert_eq!(
        first, replay,
        "fixed problem and simulation seeds must replay"
    );
    assert_eq!(first.len(), 5);
    assert_eq!(
        first
            .iter()
            .filter(|row| row.row_type == RowType::Correction)
            .count(),
        4
    );

    let schema =
        serde_json::from_str(include_str!("../../../schemas/dialogue.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut problem_ids = BTreeSet::new();
    let mut row_ids = BTreeSet::new();
    for row in &first {
        assert!(problem_ids.insert(&row.problem_id));
        assert!(row_ids.insert(&row.row_id));
        validator
            .validate(&serde_json::to_value(row).unwrap())
            .unwrap();
        row.validate()
            .expect("same semantic validator used by tutor-validate");
        check_pre_closing_tutor_turns(row);
    }
}

fn local_endpoint() -> String {
    let endpoint = std::env::var("TUTOR_MATH_ENDPOINT")
        .unwrap_or_else(|_| tutor_wire::DEFAULT_ENDPOINT.to_owned());
    assert!(
        is_local_endpoint(&endpoint),
        "use a numeric IPv4 loopback or absolute Unix socket endpoint"
    );
    endpoint
}

fn is_local_endpoint(endpoint: &str) -> bool {
    let loopback = endpoint
        .strip_prefix("tcp://")
        .and_then(|addr| addr.parse::<SocketAddrV4>().ok())
        .is_some_and(|addr| addr.ip().is_loopback());
    let unix_socket = endpoint
        .strip_prefix("ipc://")
        .is_some_and(|path| Path::new(path).is_absolute());
    loopback || unix_socket
}

fn build_fixture(root: &Path, endpoint: &str) -> Vec<DialogueRow> {
    CASES
        .iter()
        .enumerate()
        .map(|(index, case)| {
            let out = root.join(index.to_string());
            let (problem, problem_path) = generate_problem(case, &out.join("problems"), endpoint);
            let attempt = simulate_attempt(case, &out.join("attempts"), &problem, &problem_path);
            let row = dialogue(problem, attempt);
            let expected_levels: &[u8] = match (case.misconception, case.persistence) {
                (None, _) => &[],
                (Some(_), Persistence::CorrectsAfterL1) => &[1],
                (Some(_), Persistence::CorrectsAfterL2) => &[1, 2],
                _ => panic!("this fixture only covers correction through L2"),
            };
            assert_eq!(row.hint_levels_reached, expected_levels, "{}", row.row_id);
            row
        })
        .collect()
}

fn generate_problem(case: &FixtureCase, out: &Path, endpoint: &str) -> (Problem, PathBuf) {
    run_cli(&[
        "run",
        "--generator",
        case.generator,
        "--seed-start",
        case.seed,
        "--count",
        "1",
        "--endpoint",
        endpoint,
        "--timeout-secs",
        "30",
        "--out",
        path_str(out),
    ]);
    assert_empty(&out.join("rejected/rejections.jsonl"));
    let problems = out.join("verified/problems.jsonl");
    let problem = Problem::from_json(&only_line(&problems)).unwrap();
    assert_eq!(problem.generator.name, case.generator);
    assert_eq!(problem.generator.seed, case.seed.parse::<u64>().unwrap());
    let method = match case.generator {
        "binomial_derivative_at_point" => JULIA_RECOMPUTE,
        _ => RUST_RECOMPUTE,
    };
    assert_eq!(problem.verification.as_ref().unwrap().method, method);
    (problem, problems)
}

fn simulate_attempt(
    case: &FixtureCase,
    out: &Path,
    problem: &Problem,
    problems: &Path,
) -> StudentAttempt {
    let mode = if case.misconception.is_some() {
        "correction"
    } else {
        "confirmation"
    };
    let persistence = match case.persistence {
        Persistence::CorrectsAfterL1 => "corrects_after_l1",
        Persistence::CorrectsAfterL2 => "corrects_after_l2",
        Persistence::NeedsL3 => "needs_l3",
    };
    let mut args = vec![
        "simulate",
        "--problems",
        path_str(problems),
        "--out",
        path_str(out),
        "--seed-start",
        "7",
        "--mode",
        mode,
        "--persistence",
        persistence,
        "--level",
        "typical",
        "--surface-style",
        "terse",
    ];
    if let Some(id) = case.misconception {
        args.extend(["--misconception", id.as_str()]);
    }
    run_cli(&args);
    assert_empty(&out.join("rejections.jsonl"));
    let attempt = StudentAttempt::from_json(&only_line(&out.join("attempts.jsonl"))).unwrap();
    assert_eq!(attempt.problem_id, problem.problem_id);
    assert_eq!(attempt.topic, problem.topic);
    assert_eq!(attempt.misconception, case.misconception);
    assert_eq!(attempt.correct, case.misconception.is_none());
    assert_eq!(
        attempt.persistence, case.persistence,
        "{}",
        problem.problem_id
    );
    assert_attempt_answers(problem, &attempt);
    attempt
}

fn assert_attempt_answers(problem: &Problem, attempt: &StudentAttempt) {
    assert_eq!(
        attempt.teacher_variety.status,
        TeacherVarietyStatus::Disabled
    );
    assert!(attempt.teacher_variety.teacher.is_none());
    let Answer::Integer { value } = problem.answer;
    let (final_turn, earlier) = attempt
        .student_turns
        .split_last()
        .expect("student attempt has a final turn");
    assert_eq!(
        rendered_answer(&final_turn.content),
        Some(value),
        "{}",
        final_turn.content
    );
    if let Some(transformation) = &attempt.transformation {
        assert_eq!(transformation.correct_value, value);
        assert_ne!(transformation.corrupted_value, value);
        assert_planted_answers(earlier, transformation.corrupted_value);
    }
}

fn assert_planted_answers(before_final: &[Turn], expected: i64) {
    for (index, turn) in before_final.iter().enumerate() {
        assert_eq!(
            rendered_answer(&turn.content),
            Some(expected),
            "student turn {index} should still show the planted error: {}",
            turn.content
        );
    }
}

// The terse fixture ends with an equality or a "still get <value>" retry.
// Compare the terminal answer, not an intermediate number in the working.
fn rendered_answer(content: &str) -> Option<i64> {
    let (_, number) = content
        .rsplit_once(" = ")
        .or_else(|| content.rsplit_once(" get "))?;
    number.trim_end().strip_suffix('.')?.parse().ok()
}

#[test]
fn terse_turn_parser_detects_equations_and_retries() {
    assert_eq!(
        rendered_answer("Let me redo it. h′(1) = 12 + (-18) = -6."),
        Some(-6)
    );
    assert_ne!(rendered_answer("h′(1) = 12 + (-18) = -6."), Some(12));
    assert_eq!(
        rendered_answer("I thought I would still get -12."),
        Some(-12)
    );
    assert_eq!(rendered_answer("I am not sure."), None);
}

#[test]
fn endpoint_matches_native_worker_transport() {
    assert!(is_local_endpoint("tcp://127.0.0.1:5557"));
    #[cfg(unix)]
    assert!(is_local_endpoint("ipc:///tmp/tutor.sock"));
    assert!(!is_local_endpoint("tcp://[::1]:5557"));
    assert!(!is_local_endpoint("tcp://192.0.2.1:5557"));
    assert!(!is_local_endpoint("ipc://relative.sock"));
}

struct Lesson {
    diagnosis: &'static str,
    nudge: &'static str,
    question: &'static str,
    rule: &'static str,
    follow_up: &'static str,
}

fn lesson(misconception: Option<MisconceptionId>) -> Lesson {
    let (diagnosis, nudge, question, rule, follow_up) = match misconception {
        Some(MisconceptionId::DerivativesProductRule) => (
            "You differentiated both factors correctly. When the first factor changes, what happens to the second factor in that term?",
            "Look at the step where you multiplied the two derivatives.",
            "If you differentiate just one factor at a time, which undifferentiated factor should multiply each derivative?",
            "Differentiate one factor at a time, multiply by the untouched factor, and add the two terms.",
            "Find the derivative of (x² + x)(x³ − x) at x = 1.",
        ),
        Some(MisconceptionId::DerivativesChainRule) => (
            "You evaluated the outer derivative at the inner value. Where does the rate of change of the inner function enter?",
            "Check what factor is missing from the outer derivative.",
            "At the given point, what derivative belongs to the inner function, and where does it enter the product?",
            "Multiply the outer derivative at the inner value by the inner derivative at the original point.",
            "Find the derivative of (x² + x)³ at x = 3.",
        ),
        None => (
            "",
            "",
            "",
            "Apply the power rule term by term, then substitute the requested point.",
            "Find the derivative of 2x⁵ − 3x³ + x at x = 2.",
        ),
        Some(other) => panic!("no local dialogue fixture for {other:?}"),
    };
    Lesson {
        diagnosis,
        nudge,
        question,
        rule,
        follow_up,
    }
}

// Only the smoke test assembles these rows. Student turns come from the actual simulator.
fn dialogue(problem: Problem, attempt: StudentAttempt) -> DialogueRow {
    let lesson = lesson(attempt.misconception);
    let replies = tutor_replies(&problem, &attempt, &lesson);
    let hint_levels_reached = replies.iter().filter_map(|turn| turn.hint_level).collect();
    let turns = weave_turns(attempt.student_turns, replies);
    DialogueRow {
        schema_version: DIALOGUE_SCHEMA_VERSION,
        row_id: format!("pilot-smoke/{}", attempt.attempt_id),
        row_type: if attempt.correct {
            RowType::Confirmation
        } else {
            RowType::Correction
        },
        problem_id: problem.problem_id,
        topic: problem.topic,
        system_prompt_version: TUTOR_POLICY_VERSION,
        misconceptions: attempt.misconception.into_iter().collect(),
        hint_levels_reached,
        turns,
        provenance: DialogueProvenance {
            source: RowSource::Generated,
            taxonomy_version: TAXONOMY_VERSION,
            generator: Some(NamedVersion {
                name: "procedural-pilot-smoke".to_owned(),
                version: 1,
            }),
        },
        gold: false,
    }
}

fn tutor_turn(kind: TurnKind, content: &str, hint_level: Option<u8>) -> Turn {
    Turn {
        role: Role::Tutor,
        content: content.to_owned(),
        kind: Some(kind),
        hint_level,
        reveals_final_answer: kind == TurnKind::Closing,
    }
}

fn tutor_replies(problem: &Problem, attempt: &StudentAttempt, lesson: &Lesson) -> Vec<Turn> {
    let mut replies = correction_replies(attempt, lesson);
    let Answer::Integer { value } = problem.answer;
    let symbol = if attempt.correct { "f" } else { "h" };
    let point = problem.parameters.point();
    let closing = format!(
        "Confirmed: {symbol}′({point}) = {value}. {} Try one: {}",
        lesson.rule, lesson.follow_up
    );
    replies.push(tutor_turn(TurnKind::Closing, &closing, None));
    replies
}

fn correction_replies(attempt: &StudentAttempt, lesson: &Lesson) -> Vec<Turn> {
    if attempt.correct {
        return Vec::new();
    }
    let mut replies = vec![
        tutor_turn(TurnKind::Diagnosis, lesson.diagnosis, None),
        tutor_turn(TurnKind::Hint, lesson.nudge, Some(1)),
    ];
    if attempt.persistence == Persistence::CorrectsAfterL2 {
        replies.push(tutor_turn(TurnKind::Hint, lesson.question, Some(2)));
    }
    replies
}

fn weave_turns(students: Vec<Turn>, tutors: Vec<Turn>) -> Vec<Turn> {
    assert_eq!(
        students.len(),
        tutors.len(),
        "each student turn needs one tutor response"
    );
    let mut turns = vec![Turn {
        role: Role::System,
        content: SYSTEM_PROMPT.trim_end().to_owned(),
        kind: None,
        hint_level: None,
        reveals_final_answer: false,
    }];
    turns.extend(
        students
            .into_iter()
            .zip(tutors)
            .flat_map(|(student, tutor)| [student, tutor]),
    );
    turns
}

fn check_pre_closing_tutor_turns(row: &DialogueRow) {
    let (last, earlier) = row
        .turns
        .split_last()
        .expect("validated dialogue has a closing turn");
    assert!(last.reveals_final_answer);
    for turn in earlier.iter().filter(|turn| turn.role == Role::Tutor) {
        assert!(!turn.reveals_final_answer);
        // Fixture-specific guard: the diagnosis and hints are parameter-free
        // prose. Refuse any numeric result interpolated into those turns.
        assert!(
            !turn.content.chars().any(char::is_numeric),
            "{}",
            turn.content
        );
    }
}

fn run_cli(args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_tutor-generate"))
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .args(args)
        .output()
        .expect("run tutor-generate");
    if !output.status.success() {
        let reports = args
            .iter()
            .position(|arg| *arg == "--out")
            .and_then(|i| args.get(i + 1))
            .map(Path::new)
            .map(|root| {
                ["rejected/rejections.jsonl", "rejections.jsonl"]
                    .into_iter()
                    .find_map(|path| fs::read_to_string(root.join(path)).ok())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        panic!(
            "{args:?}: {}\n{}",
            String::from_utf8_lossy(&output.stderr),
            reports
        );
    }
}

fn only_line(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    assert_eq!(text.lines().count(), 1, "expected one record in {path:?}");
    text
}

// Both CLI writers create a rejection ledger on success, even for zero records.
fn assert_empty(path: &Path) {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("could not read CLI rejection ledger {path:?}: {error}"));
    assert!(content.is_empty(), "{path:?} has rejected records");
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("UTF-8 scratch path")
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("tutor-pilot-smoke-{}-{now}", std::process::id()));
        fs::create_dir(&path).expect("create isolated fixture directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
