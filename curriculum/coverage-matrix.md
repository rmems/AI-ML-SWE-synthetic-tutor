# Curriculum coverage matrix

Owned by [#16](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/16).
This file is the coverage index for the whole repo: **one row per curriculum
topic, and the place where dataset coverage is counted**. A generation spec
may only target topics that exist here (the
[generation-spec form](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/new?template=generation-spec.yml)
and the
[taxonomy-entry form](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/new?template=taxonomy-entry.yml)
both preflight against this matrix). Personal-study notes and demos do not
belong here — coverage means *trainable* coverage, not a title.

Topic IDs are the `topic` field of `schemas/problem.schema.json`: dotted IDs
matching `^[a-z0-9_]+(\.[a-z0-9_]+)*$`, now fixed by this file (no longer
provisional). Misconception IDs live in `schemas/taxonomy.yaml`
(documented in `docs/taxonomy.md`); **no ID may appear below that is not
already in the taxonomy** — propose new IDs with the taxonomy-entry form
first.

## Columns

    topic | domain | prerequisite topics | misconception ids available |
    problem generator status | target rows | generated rows | eval items held out

Two extra columns are kept because the generation-spec form needs them at a
glance: `generators / verification` (the independent check each problem type
must pass — always on the opposite side of the Rust↔Julia ZMQ boundary from
the generator, so a generator never grades itself) and `family / slice`
(which GitHub issue owns the row). Every row cites its family and/or slice
issue; the [family / slice index](#family--slice-index) below links the same
issues back.

## Status vocabulary

Every row has exactly one status, from `problem generator status` column:

| Status | Meaning |
|---|---|
| `generator + verification` | Seeded generators exist **and** each has an independent verification path. Trainable. |
| `planned` | Named seed topic of a filed family or slice issue, but no generator / not yet trainable. |
| `specced` | In scope of a filed family (taxonomy reference, prerequisite, or family-scope placeholder) but not a named seed; still not generating. |
| `unscoped` | Named placeholder (post-pilot AI topics, later-language families, v2/v3/v4/v5 lists) with no family issue or no taxonomy/verification yet. **Unscoped rows must never be marked generating.** |

**Gate to generating:** a topic with no misconception ids and no verification
method cannot move to `generator + verification` — both must land first
(taxonomy entry + independent verifier).

## Counting rules

- Do not hand-edit the `generated rows` or `eval items held out` columns.
  No committed release split manifest exists yet, so both columns are `0`
  on every row.
- When a release manifest is written by the [#8](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/8)
  builder (`tutor-build`, PR #43), `generated rows` is read from
  `counts.topics` (accepted rows per topic per split). That tally is
  whatever the build ingested and accepted — including gold or other
  hand-written rows if they were passed as `--rows` — which is why the
  matrix still does not invent numbers by hand.
- `eval items held out` cannot yet be read as a per-topic item count:
  the current manifest records held-out *file* digests under
  `inputs.heldout` (path, sha256, bytes), not a count of held-out items
  per topic. That column stays `0` until the manifest or `evals/heldout/`
  exposes an item-level count.
- `target rows` is only filled where an existing issue states a number.
  The only stated target today: [#20](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/20)
  is **300 dialogues for the derivative-misconceptions family as a whole —
  not 300 per topic** (the three pilot topics share the 300).

## Family / slice index

Every family / slice issue appears below and is cited by its topic rows.

| Issue | Family / slice | Status here |
|---|---|---|
| [#20](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/20) | Pilot dataset: derivative misconceptions (first end-to-end family) | 3 topics at `generator + verification` |
| [#10](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/10) | Dataset family: precalculus tutoring dialogues | rows below, `planned`/`specced` |
| [#14](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/14) | Dataset slice: function transformations (first precalculus slice) | `functions.transformations`, `planned` |
| [#11](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/11) | Dataset family: Calculus I tutoring dialogues | rows below, `planned`/`specced` |
| [#15](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/15) | Dataset slice: derivative definition — secant line to tangent line | `calculus.derivatives.definition`, `planned` |
| [#12](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/12) | Dataset family: ML-math tutoring dialogues (bridge to later language datasets) | rows below, `planned` |
| [#28](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/28) | Dataset family: Calculus II | scope placeholder, `specced` |
| [#29](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/29) | Dataset family: Calculus III | scope placeholder, `specced` |
| [#30](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/30) | Dataset family: Linear Algebra | scope placeholder, `specced` |
| [#31](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/31) | Dataset family: Discrete Mathematics | scope placeholder, `specced` |
| [#32](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/32) | Dataset family: Probability & Statistics | scope placeholder, `specced` |
| [#33](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/33) | Dataset family: Differential Equations | scope placeholder, `specced` |
| [#35](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/35) | Epic: v2 — AI/ML/SWE expansion, standard level | placeholder rows, `unscoped` |
| [#36](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/36) | Epic: v3 — advanced topics & depth | placeholder rows, `unscoped` |
| [#37](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/37) | Epic: v4 — image datasets (modality ladder) | placeholder row, `unscoped` |
| [#38](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/38) | Epic: v5 — voice & speech datasets (modality ladder) | placeholder row, `unscoped` |

## v1 — Mathematics foundations

The Nov 25 curriculum map (Raul, comment on #16) fixes the shape: the tutor
teaches the AI/ML/SWE curriculum as a whole; **mathematics is v1's starting
track, not the project's scope**. v1 is eight math subjects — precalculus,
Calculus I–III, linear algebra, discrete mathematics, probability &
statistics, differential equations — with the family issues above. The ML-math
family (#12) is an applied selection drawn from linear algebra, probability &
statistics, and Calculus III, and is the bridge to the later language
datasets.

### Pilot — derivative misconceptions ([#20](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/20))

The three pilot rows stay first and fully filled in. Verification is
cross-boundary: Julia (`math-julia`) generators are checked by the Rust
`tutor-verify` side; the Rust generator (`binomial_derivative_at_point`) is
checked by Julia `verify_answer` over ZMQ — the generator never grades itself.
Target: **300 dialogues for the family as a whole** (shared across the three
topics), per #20. Generated rows and eval items held out are `0` until split
manifests land with #8.

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `calculus.derivatives.power_rule` (power rule) | math | `calculus.derivatives.definition`, `algebra.exponent_rules` | `derivatives.power_rule_overgeneralised`, `algebra.sign_error` | generator + verification | `polynomial_derivative_at_point` (math-julia), `binomial_derivative_at_point` (tutor-generate) → cross-boundary verify (Rust `tutor-verify` / Julia `verify_answer`) | #20 pilot · #11 | 300 dialogues for the #20 family as a whole (not per topic) | 0 — from split manifests once #8 lands | 0 — from split manifests once #8 lands |
| `calculus.derivatives.product_rule` (product rule) | math | `calculus.derivatives.power_rule` | `derivatives.product_rule`, `algebra.sign_error` | generator + verification | `product_rule_at_point` (math-julia) → Rust `tutor-verify` | #20 pilot · #11 | (shared #20 family target) | 0 — from split manifests once #8 lands | 0 — from split manifests once #8 lands |
| `calculus.derivatives.chain_rule` (chain rule) | math | `calculus.derivatives.power_rule`, `functions.composition` | `derivatives.chain_rule`, `algebra.sign_error` | generator + verification | `chain_rule_at_point` (math-julia) → Rust `tutor-verify` | #20 pilot · #11 | (shared #20 family target) | 0 — from split manifests once #8 lands | 0 — from split manifests once #8 lands |

### Precalculus ([#10](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/10)) and its first slice — function transformations ([#14](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/14))

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `algebra.rational_expressions` (fractions & rational expressions) | math | — | `limits.divide_by_zero_form` | planned | — | #10 | — | 0 | 0 |
| `algebra.exponent_rules` (exponents) | math | — | `algebra.exponent_rule` | planned | — | #10 | — | 0 | 0 |
| `algebra.radicals` (radicals) | math | `algebra.exponent_rules` | — | planned | — | #10 | — | 0 | 0 |
| `algebra.logarithms` (logarithms) | math | `algebra.exponent_rules` | — | planned | — | #10 | — | 0 | 0 |
| `algebra.combining_like_terms` (combining like terms) | math | — | `algebra.like_terms` | specced | — | #10 | — | 0 | 0 |
| `algebra.distributive_property` (distributive property) | math | — | `algebra.distribution_error` | specced | — | #10 | — | 0 | 0 |
| `algebra.polynomial_expansion` (polynomial expansion) | math | `algebra.distributive_property` | `algebra.distribution_error` | specced | — | #10 | — | 0 | 0 |
| `algebra.polynomial_operations` (polynomial operations) | math | `algebra.exponent_rules`, `algebra.combining_like_terms` | `algebra.exponent_rule` | specced | — | #10 | — | 0 | 0 |
| `algebra.linear_equations` (linear equations) | math | `algebra.combining_like_terms` | `algebra.like_terms`, `algebra.sign_error` | planned | — | #10 | — | 0 | 0 |
| `algebra.multi_step_equations` (multi-step equations) | math | `algebra.linear_equations` | `algebra.sign_error` | planned | — | #10 | — | 0 | 0 |
| `algebra.inequalities` (inequalities) | math | `algebra.linear_equations` | — | planned | — | #10 | — | 0 | 0 |
| `functions.function_notation` (function notation) | math | `algebra.linear_equations` | `functions.composition_order`, `functions.inverse_not_reciprocal` | planned | — | #10 | — | 0 | 0 |
| `functions.domain` (domain) | math | `functions.function_notation` | `functions.domain_range` | planned | — | #10 | — | 0 | 0 |
| `functions.range` (range) | math | `functions.function_notation` | `functions.domain_range` | planned | — | #10 | — | 0 | 0 |
| `functions.rational_functions` (rational functions) | math | `functions.domain`, `algebra.rational_expressions` | `functions.domain_range` | specced | — | #10 | — | 0 | 0 |
| `functions.composition` (composition) | math | `functions.function_notation` | `functions.composition_order` | planned | — | #10 | — | 0 | 0 |
| `functions.inverse_functions` (inverse functions) | math | `functions.function_notation`, `functions.composition` | `functions.inverse_not_reciprocal` | planned | — | #10 | — | 0 | 0 |
| `functions.piecewise_functions` (piecewise functions) | math | `functions.function_notation`, `functions.domain` | `limits.one_sided_limit` | specced | — | #10 | — | 0 | 0 |
| `functions.transformations` (graph / function transformations) | math | `functions.function_notation` | — | planned | — | #14 slice · #10 | — | 0 | 0 |
| `trig.radians` (radians) | math | — | — | planned | — | #10 | — | 0 | 0 |
| `trig.unit_circle` (unit circle) | math | `trig.radians` | `trig.unit_circle` | planned | — | #10 | — | 0 | 0 |
| `trig.trigonometric_functions` (trig functions) | math | `trig.unit_circle` | `trig.unit_circle` | planned | — | #10 | — | 0 | 0 |
| `trig.special_angles` (special angles) | math | `trig.unit_circle` | `trig.unit_circle` | specced | — | #10 | — | 0 | 0 |
| `trig.tangent` (tangent) | math | `trig.trigonometric_functions` | `trig.identity_confusion` | specced | — | #10 | — | 0 | 0 |
| `trig.identities` (trig identities) | math | `trig.trigonometric_functions` | `trig.identity_confusion` | planned | — | #10 | — | 0 | 0 |
| `trig.pythagorean_identity` (Pythagorean identity) | math | `trig.trigonometric_functions` | `trig.identity_confusion` | planned | — | #10 | — | 0 | 0 |

### Calculus I ([#11](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/11)) and its slice — derivative definition, secant to tangent ([#15](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/15))

The secant-line-to-tangent-line picture is a **problem family** here
(`calculus.derivatives.definition`, slice #15), not a standalone demo.

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `limits.limit_evaluation` (intuitive limits & evaluation) | math | `functions.function_notation` | `limits.divide_by_zero_form` | planned | — | #11 | — | 0 | 0 |
| `limits.one_sided_limits` (one-sided limits) | math | `limits.limit_evaluation` | `limits.one_sided_limit` | planned | — | #11 | — | 0 | 0 |
| `limits.limit_existence` (limit existence) | math | `limits.one_sided_limits` | `limits.one_sided_limit` | specced | — | #11 | — | 0 | 0 |
| `limits.limit_laws` (limit laws) | math | `limits.limit_evaluation` | `limits.infinity_indeterminate_form` | planned | — | #11 | — | 0 | 0 |
| `limits.indeterminate_forms` (indeterminate forms) | math | `limits.limit_evaluation` | `limits.divide_by_zero_form`, `limits.infinity_indeterminate_form` | specced | — | #11 | — | 0 | 0 |
| `limits.limits_at_infinity` (limits at infinity) | math | `limits.limit_laws` | `limits.infinity_indeterminate_form` | specced | — | #11 | — | 0 | 0 |
| `limits.continuity` (continuity) | math | `limits.limit_existence` | — | planned | — | #11 | — | 0 | 0 |
| `calculus.derivatives.definition` (derivative definition — secant to tangent) | math | `limits.limit_evaluation`, `limits.limit_existence` | — | planned | — | #15 slice · #11 | — | 0 | 0 |
| `calculus.derivatives.composite_functions` (composite functions) | math | `functions.composition`, `calculus.derivatives.power_rule` | `derivatives.chain_rule` | specced | — | #11 | — | 0 | 0 |
| `calculus.derivatives.quotient_rule` (quotient rule) | math | `calculus.derivatives.power_rule` | `derivatives.quotient_rule` | planned | — | #11 | — | 0 | 0 |
| `calculus.derivatives.trigonometric_derivatives` (trigonometric derivatives) | math | `calculus.derivatives.power_rule`, `trig.trigonometric_functions` | `derivatives.product_rule` | specced | — | #11 | — | 0 | 0 |
| `calculus.derivatives.exponential_derivatives` (exponential derivatives) | math | `calculus.derivatives.power_rule`, `algebra.logarithms` | `derivatives.power_rule_overgeneralised` | specced | — | #11 | — | 0 | 0 |
| `calculus.derivatives.rational_functions` (derivatives of rational functions) | math | `calculus.derivatives.quotient_rule`, `functions.rational_functions` | `derivatives.quotient_rule` | specced | — | #11 | — | 0 | 0 |
| `calculus.derivatives.implicit_differentiation` (implicit differentiation) | math | `calculus.derivatives.chain_rule` | `derivatives.implicit_differentiation` | planned | — | #11 | — | 0 | 0 |
| `calculus.derivatives.tangent_lines` (tangent lines) | math | `calculus.derivatives.definition`, `calculus.derivatives.power_rule` | — | planned | — | #11 | — | 0 | 0 |
| `calculus.derivatives.related_rates` (related rates) | math | `calculus.derivatives.implicit_differentiation` | — | planned | — | #11 | — | 0 | 0 |
| `calculus.derivatives.optimisation` (optimisation) | math | `calculus.derivatives.tangent_lines` | — | planned | — | #11 | — | 0 | 0 |
| `calculus.derivatives.curve_sketching` (curve sketching) | math | `calculus.derivatives.tangent_lines` | — | planned | — | #11 | — | 0 | 0 |
| `integrals.antiderivatives` (antiderivatives) | math | `calculus.derivatives.power_rule` | `integrals.constant_of_integration` | planned | — | #11 | — | 0 | 0 |
| `integrals.basic_antiderivatives` (basic antiderivatives) | math | `integrals.antiderivatives` | `integrals.antiderivative_exponent` | planned | — | #11 | — | 0 | 0 |
| `integrals.power_rule` (power rule for integrals) | math | `integrals.antiderivatives`, `calculus.derivatives.power_rule` | `integrals.antiderivative_exponent` | specced | — | #11 | — | 0 | 0 |
| `integrals.indefinite_integrals` (indefinite integrals) | math | `integrals.basic_antiderivatives` | `integrals.constant_of_integration` | specced | — | #11 | — | 0 | 0 |
| `integrals.definite_integrals` (definite integrals — area under a curve) | math | `integrals.indefinite_integrals` | `integrals.setup_confusion` | planned | — | #11 | — | 0 | 0 |
| `integrals.fundamental_theorem_of_calculus` (fundamental theorem preview) | math | `integrals.definite_integrals` | `integrals.setup_confusion` | planned | — | #11 | — | 0 | 0 |

### ML math ([#12](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/12))

Applied selection drawn from linear algebra, probability & statistics, and
Calculus III — and the bridge to the later language datasets (the family
already touches Python).

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `linalg.vectors_norms` (vectors and norms) | ML math | — | — | planned | — | #12 | — | 0 | 0 |
| `linalg.matrix_multiplication` (matrix multiplication) | ML math | `linalg.vectors_norms` | — | planned | — | #12 | — | 0 | 0 |
| `ml_math.numerical_derivative` (numerical derivative) | ML math | `calculus.derivatives.definition` | — | planned | — | #12 | — | 0 | 0 |
| `ml_math.gradient_descent` (gradient descent, math view) | ML math | `ml_math.numerical_derivative`, `linalg.vectors_norms` | — | planned | — | #12 | — | 0 | 0 |
| `probability.random_variables` (random variables) | ML math | — | — | planned | — | #12 | — | 0 | 0 |
| `probability.distributions` (distributions) | ML math | `probability.random_variables` | — | planned | — | #12 | — | 0 | 0 |

### v1 families awaiting topic enumeration

Filed 2026-11-25 (official seed content per the comment on #16). Each family
gets one scope-placeholder row until the family issue enumerates real topics;
the placeholder is *not* a curriculum topic and stays `specced` — replace it
with real topic rows when the family's seed list lands.

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `calculus_ii.scope` (Calculus II — family scope placeholder) | math | `integrals.fundamental_theorem_of_calculus` | — | specced | — | #28 | — | 0 | 0 |
| `calculus_iii.scope` (Calculus III — family scope placeholder) | math | `calculus_ii.scope` | — | specced | — | #29 | — | 0 | 0 |
| `linear_algebra.scope` (Linear Algebra — family scope placeholder) | math | — | — | specced | — | #30 | — | 0 | 0 |
| `discrete_math.scope` (Discrete Mathematics — family scope placeholder) | math | — | — | specced | — | #31 | — | 0 | 0 |
| `probability_statistics.scope` (Probability & Statistics — family scope placeholder) | math | — | — | specced | — | #32 | — | 0 | 0 |
| `differential_equations.scope` (Differential Equations — family scope placeholder) | math | `calculus_ii.scope` | — | specced | — | #33 | — | 0 | 0 |

## Decisions

### The name gap (AI/SWE)

The repo is named **AI-ML-SWE**, but seed coverage today is **math and ML
math only**. There is no AI or SWE coverage yet — nothing below `AI / ML` or
`SWE / programming language` has a generator, a taxonomy entry, or a family
issue with topics. Do not let the repo name imply coverage that does not
exist.

### Later languages (decided 2026-11-15)

After the math families, this repo generates tutoring datasets for other
programming languages: students tutored *in* a language, with **code
misconceptions**, verified by **compiling and running the code**. Decided
default order — first target language after math is **Python** (largest
student population; the ML-math family already touches it), then **Rust**,
then **Julia**. This default can be reordered. The family rows below are
named placeholders only — no family issues or code-misconception taxonomy
exist yet, so they stay `unscoped`.

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `swe.python` (Python tutoring family — first after math, decided default) | SWE / programming language | — | — | unscoped | — | none yet (decision 2026-11-15) | — | 0 | 0 |
| `swe.rust` (Rust tutoring family — second) | SWE / programming language | — | — | unscoped | — | none yet (decision 2026-11-15) | — | 0 | 0 |
| `swe.julia` (Julia tutoring family — third) | SWE / programming language | — | — | unscoped | — | none yet (decision 2026-11-15) | — | 0 | 0 |

### Post-pilot AI topics (decision default: named but unscoped)

Placeholder rows for the first AI-tutoring topics, **post-pilot**. No taxonomy
IDs are invented for them — they need taxonomy entries, a family/slice issue,
and a verification method before any status change.

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `ai.probability_for_ml` (probability for ML) | AI / ML | `probability.distributions` | — | unscoped | — | none yet (post-pilot) | — | 0 | 0 |
| `ai.loss_functions` (loss functions) | AI / ML | `ml_math.gradient_descent` | — | unscoped | — | none yet (post-pilot) | — | 0 | 0 |
| `ai.gradient_descent_intuition` (gradient descent intuition) | AI / ML | `ml_math.gradient_descent` | — | unscoped | — | none yet (post-pilot) | — | 0 | 0 |

`ml_math.gradient_descent` (#12, `planned`) is the math treatment — gradients
as vectors of partial derivatives, the update rule. The
`ai.gradient_descent_intuition` placeholder is the broader AI-topic view
(training loop, loss surface, learning-rate intuition) and depends on the
ML-math row rather than duplicating it.

### v2 — AI/ML/SWE expansion, standard level (epic [#35](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/35))

List only — **no issues until v1's first measured result**. Placeholder rows,
`unscoped`.

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `swe.cs_i` (CS I) | SWE / programming language | — | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.cs_ii` (CS II) | SWE / programming language | `swe.cs_i` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.data_structures` (Data Structures) | SWE / programming language | `swe.cs_ii` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.algorithms` (Algorithms) | SWE / programming language | `swe.data_structures` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.computer_architecture` (Computer Architecture) | SWE / programming language | `swe.cs_i` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.operating_systems` (Operating Systems) | SWE / programming language | `swe.computer_architecture` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.databases` (Databases) | SWE / programming language | `swe.data_structures` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.software_engineering` (Software Engineering) | SWE / programming language | `swe.cs_ii` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.computer_networks` (Computer Networks) | SWE / programming language | `swe.operating_systems` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `swe.programming_languages` (Principles of Programming Languages) | SWE / programming language | `swe.data_structures` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `ai.artificial_intelligence` (AI) | AI / ML | — | — | unscoped | — | epic #35 | — | 0 | 0 |
| `ai.machine_learning` (ML) | AI / ML | `ai.probability_for_ml`, `ml_math.gradient_descent` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `ai.deep_learning` (Deep Learning) | AI / ML | `ai.machine_learning` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `ai.natural_language_processing` (NLP) | AI / ML | `ai.deep_learning` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `ai.computer_vision` (Computer Vision) | AI / ML | `ai.deep_learning` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `ai.generative_ai` (Generative AI) | AI / ML | `ai.deep_learning` | — | unscoped | — | epic #35 | — | 0 | 0 |
| `ai.data_analytics` (Data Analytics / Data Science for AI) | AI / ML | `ai.machine_learning` | — | unscoped | — | epic #35 | — | 0 | 0 |

### v3 — Advanced topics & depth (epic [#36](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/36))

List only. Placeholder rows, `unscoped`.

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `math.real_analysis` (real analysis) | math | `calculus_iii.scope` | — | unscoped | — | epic #36 | — | 0 | 0 |
| `math.multivariable_depth` (multivariable depth) | math | `calculus_iii.scope` | — | unscoped | — | epic #36 | — | 0 | 0 |
| `ai.reinforcement_learning` (reinforcement learning) | AI / ML | `ai.machine_learning`, `ai.probabilistic_ml` | — | unscoped | — | epic #36 | — | 0 | 0 |
| `ai.probabilistic_ml` (probabilistic ML) | AI / ML | `ai.machine_learning`, `probability_statistics.scope` | — | unscoped | — | epic #36 | — | 0 | 0 |
| `swe.distributed_systems` (distributed systems) | SWE / programming language | `swe.operating_systems`, `swe.computer_networks` | — | unscoped | — | epic #36 | — | 0 | 0 |
| `swe.compilers` (compilers) | SWE / programming language | `swe.programming_languages`, `swe.computer_architecture` | — | unscoped | — | epic #36 | — | 0 | 0 |

### Modality ladder (unchanged)

v4 image datasets (epic [#37](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/37));
v5 voice & speech datasets (epic [#38](https://github.com/rmems/AI-ML-SWE-synthetic-tutor/issues/38)).
Tutor speech output is a TTS renderer, **not** a dataset concern. Index-level
placeholder rows only — `unscoped`.

| Topic | Domain | Prerequisite topics | Misconception ids available | Problem generator status | Generators / verification | Family / slice | Target rows | Generated rows | Eval items held out |
|---|---|---|---|---|---|---|---|---|---|
| `modality.image_datasets` (v4 image datasets) | modality | — | — | unscoped | — | epic #37 | — | 0 | 0 |
| `modality.voice_speech` (v5 voice & speech datasets) | modality | — | — | unscoped | — | epic #38 | — | 0 | 0 |
