//! Test-only proof for the quadratic secant-slope prerequisite of RM-2498.
//! Neither these problems nor their proof frames enter the generation pipeline.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64;
use tutor_generate::student::format_polynomial;
use tutor_schema::{MAX_SEED, MisconceptionId};

const TOPIC: &str = "calculus.derivatives.definition";
const TAXONOMY: [MisconceptionId; 3] = [
    MisconceptionId::LimitsDivideByZeroForm,
    MisconceptionId::DerivativesSecantAsTangent,
    MisconceptionId::DerivativesUndefinedQuotientImpliesNoLimit,
];
const DENOMINATORS: [i64; 4] = [1, 2, 4, 8];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Quadratic {
    quadratic: i64,
    linear: i64,
    constant: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SecantSample {
    // h = sign / denominator. Slope and absolute error share that denominator.
    sign: i64,
    denominator: i64,
    slope_numerator: i64,
    abs_error_numerator: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LimitWitness {
    // For nonzero h the quotient equals value + h_coefficient * h.
    value: i64,
    h_coefficient: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SecantProblem {
    topic: &'static str,
    eligible_misconceptions: [MisconceptionId; 3],
    function: Quadratic,
    at: i64,
    limit: LimitWitness,
    samples: Vec<SecantSample>,
}

fn draw(seed: u64) -> Option<SecantProblem> {
    if seed > MAX_SEED {
        return None;
    }
    let mut rng = Pcg64::seed_from_u64(seed);
    let nonzero = [-4, -3, -2, -1, 1, 2, 3, 4];
    let function = Quadratic {
        quadratic: nonzero[rng.random_range(0..nonzero.len())],
        linear: rng.random_range(-5..=5),
        constant: rng.random_range(-5..=5),
    };
    let at = rng.random_range(-4..=4);
    Some(quadratic_problem(function, at))
}

fn quadratic_problem(function: Quadratic, at: i64) -> SecantProblem {
    // Expand (f(at+h) - f(at))/h exactly for a quadratic. The Julia verifier
    // uses direct substitution and central differences, not this expansion.
    let limit = LimitWitness {
        value: 2 * function.quadratic * at + function.linear,
        h_coefficient: function.quadratic,
    };
    let samples = DENOMINATORS
        .into_iter()
        .flat_map(|denominator| [-1, 1].map(|sign| sample(limit, sign, denominator)))
        .collect();
    SecantProblem {
        topic: TOPIC,
        eligible_misconceptions: TAXONOMY,
        function,
        at,
        limit,
        samples,
    }
}

fn sample(limit: LimitWitness, sign: i64, denominator: i64) -> SecantSample {
    let offset_numerator = limit.h_coefficient * sign;
    SecantSample {
        sign,
        denominator,
        slope_numerator: limit.value * denominator + offset_numerator,
        abs_error_numerator: offset_numerator.abs(),
    }
}

impl SecantProblem {
    fn statement(&self) -> String {
        let Quadratic {
            quadratic,
            linear,
            constant,
        } = self.function;
        let polynomial = format_polynomial(&[constant, linear, quadratic]);
        let point = self.at;
        format!(
            "For f(x) = {polynomial}, use secant slopes on both sides of x = {point}.              What derivative do the slopes approach as the nonzero steps shrink, and why?"
        )
    }

    fn write_proof_frame(&self, mut output: impl Write) -> std::io::Result<()> {
        let Quadratic {
            quadratic,
            linear,
            constant,
        } = self.function;
        write!(
            output,
            "{quadratic} {linear} {constant} {} {} {}",
            self.at, self.limit.value, self.limit.h_coefficient
        )?;
        for s in &self.samples {
            write!(
                output,
                " {} {} {} {}",
                s.sign, s.denominator, s.slope_numerator, s.abs_error_numerator
            )?;
        }
        writeln!(output)
    }
}

#[test]
fn seeds_replay_and_classification_uses_the_secant_taxonomy() {
    for seed in [0, 1, 4, 19, MAX_SEED] {
        assert_eq!(draw(seed), draw(seed));
        let p = draw(seed).unwrap();
        assert_ne!(p.function.quadratic, 0);
        assert!(p.statement().contains("secant slopes on both sides"));
        assert_eq!(p.topic, TOPIC);
        check_taxonomy(&p);
    }
    assert_ne!(draw(1), draw(4));
    assert!(draw(MAX_SEED + 1).is_none());
}

fn check_taxonomy(problem: &SecantProblem) {
    assert_eq!(problem.eligible_misconceptions, TAXONOMY);
    for id in problem.eligible_misconceptions {
        assert!(id.topics().contains(&problem.topic));
        assert_eq!(id.introduced_for_topic(problem.topic), 2);
    }
}

#[test]
fn square_at_two_has_exact_two_sided_slopes_and_shrinking_errors() {
    let function = Quadratic {
        quadratic: 1,
        linear: 0,
        constant: 0,
    };
    let p = quadratic_problem(function, 2);
    assert_eq!(p.limit.value, 4);
    assert_eq!(p.limit.h_coefficient, 1);
    let (pairs, remainder) = p.samples.as_chunks::<2>();
    assert!(remainder.is_empty());
    let pairs: Vec<_> = pairs.iter().map(display_pair).collect();
    assert_eq!(
        pairs,
        [
            (1, [3, 5], [1, 1]),
            (2, [7, 9], [1, 1]),
            (4, [15, 17], [1, 1]),
            (8, [31, 33], [1, 1]),
        ]
    );
}

fn display_pair([left, right]: &[SecantSample; 2]) -> (i64, [i64; 2], [i64; 2]) {
    assert_eq!((left.sign, right.sign), (-1, 1));
    assert_eq!(left.denominator, right.denominator);
    (
        left.denominator,
        [left.slope_numerator, right.slope_numerator],
        [left.abs_error_numerator, right.abs_error_numerator],
    )
}

#[test]
#[ignore = "requires a local Julia executable"]
fn julia_independently_checks_seeded_tables_and_rejects_tampering() {
    let base = draw(1).unwrap();
    let mut cases: Vec<_> = [0, 1, 4, 19, MAX_SEED]
        .into_iter()
        .map(|seed| (draw(seed).unwrap(), "ok"))
        .collect();
    let mut push = |edit: fn(&mut SecantProblem), reason| {
        let mut candidate = base.clone();
        edit(&mut candidate);
        cases.push((candidate, reason));
    };
    push(|p| p.limit.value += 1, "wrong_limit");
    push(|p| p.limit.h_coefficient += 1, "wrong_h_coefficient");
    push(|p| p.samples[0].slope_numerator += 1, "wrong_slope");
    push(
        |p| p.samples[0].abs_error_numerator += 1,
        "wrong_absolute_error",
    );
    push(|p| p.samples[0].sign = 0, "invalid_grid");
    push(|p| p.samples[2].denominator = 1, "invalid_grid");
    push(|p| p.samples.swap(0, 1), "invalid_grid");
    push(
        |p| {
            p.samples.pop();
        },
        "invalid_frame",
    );
    push(|p| p.function.quadratic = 0, "not_quadratic");
    assert_eq!(
        verify_in_julia(&cases),
        cases.iter().map(|c| c.1).collect::<Vec<_>>()
    );
}

fn verify_in_julia(cases: &[(SecantProblem, &str)]) -> Vec<String> {
    let jl = std::env::var_os("JULIA_EXE").unwrap_or_else(|| "julia".into());
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../math-julia/test/quadratic_secant_verifier.jl");
    let mut child = Command::new(jl)
        .arg("--startup-file=no")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start local Julia, or set JULIA_EXE");
    let mut input = child.stdin.take().unwrap();
    for (problem, _) in cases {
        problem.write_proof_frame(&mut input).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Julia verifier failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}
