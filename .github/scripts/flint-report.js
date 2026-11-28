// Data processing for .github/workflows/flint.yml, called through actions/github-script.
const fs = require("fs");

const SUMMARY = "steel-flint/log/flint_summary.json";
const BASELINE = "baseline/flint_baseline.json";
const BASE_IMPLEMENTED = "baseline/implemented.txt";
const IMPLEMENTED = "implemented.txt";
const REGRESSIONS = "regressions.json";
const MERGED_BASELINE = "flint_baseline.json";
const REPORT_DIR = "report";
const HEADING = "## Flint test report";

const readJson = (path) => (fs.existsSync(path) ? JSON.parse(fs.readFileSync(path, "utf8")) : null);
const readLines = (path) =>
  fs.existsSync(path) ? fs.readFileSync(path, "utf8").split("\n").filter(Boolean) : null;
const sorted = (iterable) => [...iterable].sort();
const names = (entries) => entries.map((e) => e.name);

/** Tests that passed in the baseline but do not pass now (`missing`: absent from the results). */
function findRegressions(base, curr) {
  const executed = curr.filter((e) => !e.skipped);
  const present = new Set(names(executed));
  const passed = new Set(names(executed.filter((e) => e.success)));
  const required = sorted(new Set(names(base.filter((e) => e.success))));
  return required
    .filter((name) => !passed.has(name))
    .map((name) => ({ name, missing: !present.has(name) }));
}

/** Old baseline successes ∪ current successes; ids come from the latest entry. */
function mergeBaseline(base, curr) {
  const merged = new Map();
  for (const e of [...base, ...curr]) {
    merged.set(e.name, { name: e.name, ids: e.ids, success: e.success || !!merged.get(e.name)?.success });
  }
  return sorted(merged.keys()).map((name) => merged.get(name));
}

const newIds = (current, base) => sorted(current.filter((id) => !base.includes(id)));

const regressionLine = (r) => `${r.name}${r.missing ? " (missing from results)" : ""}`;
const passedCount = (tests) => tests.filter((t) => t.success && !t.skipped).length;

function newlyImplementedSection(summary, currentIds, baseIds) {
  if (!currentIds || !baseIds) return ["No implemented-id list for this run or master yet — skipped."];
  const added = newIds(currentIds, baseIds);
  if (added.length === 0) return ["None."];

  const tests = summary.filter((t) => !t.skipped && t.ids.some((id) => added.includes(id)));
  const failed = names(tests.filter((t) => !t.success));
  const lines = [
    `\`${added.join("`, `")}\``,
    "",
    `**${tests.length} tests**, ${passedCount(tests)} passed, ${failed.length} failed`,
  ];
  if (failed.length > 0) {
    lines.push("", "Failed:", ...failed.map((n) => `- :x: \`${n}\``));
  }
  if (tests.length === 0) {
    lines.push("", ":warning: No Flint tests cover these ids yet.");
  }
  return lines;
}

function buildReport({ summary, regressions, currentIds, baseIds, runUrl }) {
  const lines = [HEADING, ""];
  if (!Array.isArray(summary)) {
    lines.push(
      `:x: Flint did not produce a summary. See the [workflow run](${runUrl}) for the error.`,
    );
    return lines.join("\n");
  }

  const skippedTotal = summary.filter((test) => test.skipped).length;
  const failedTotal = summary.length - passedCount(summary) - skippedTotal;
  lines.push(
    `**Implemented tests:** ${summary.length} total, ${passedCount(summary)} passed, ${failedTotal} failed, ${skippedTotal} skipped`,
  );
  const errors = summary.filter((test) => test.error);
  if (errors.length > 0) {
    lines.push("", "### Errors", ...errors.map((test) => `- :x: \`${test.name}\`: ${test.error}`));
  }
  const actualRegressions = regressions?.filter((test) => !test.missing) ?? [];
  const missing = regressions?.filter((test) => test.missing) ?? [];
  lines.push("", "### Regressions");
  if (!regressions) {
    lines.push("No baseline comparison available. See the workflow run for details.");
  } else if (regressions.length === 0) {
    lines.push("None :white_check_mark:");
  } else if (actualRegressions.length === 0) {
    lines.push("None among executed tests (:warning: some baseline tests did not run).");
  } else {
    lines.push(...actualRegressions.map((test) => `- :x: \`${test.name}\``));
  }
  if (missing.length > 0) {
    const panicked = errors.some((test) => test.error === "Flint test panicked");
    lines.push("", "### Baseline tests that did not run");
    if (panicked) {
      lines.push("The test suite aborted early after a panic, so these baseline tests did not run:");
    }
    lines.push(...missing.map((test) => `- \`${test.name}\``));
  }
  lines.push("", "### Newly implemented blocks/items", ...newlyImplementedSection(summary, currentIds, baseIds));
  lines.push(
    "",
    `<sub>[Workflow run](${runUrl}) · failure links to flint-viz are in the test log</sub>`,
  );
  return lines.join("\n");
}

function validateInputs({ core }) {
  const curr = readJson(SUMMARY);
  if (!Array.isArray(curr) || !curr.some((test) => !test.skipped)) {
    core.setFailed("No Flint tests executed or the run did not produce a summary");
    return;
  }
  if (new Set(names(curr)).size !== curr.length) {
    core.setFailed("Flint test names must be unique");
    return;
  }

  const benchmarkSha = process.env.FLINT_BENCHMARK_SHA;
  if (!/^[a-f0-9]{40}$/.test(benchmarkSha ?? "")) {
    core.setFailed("Missing FlintBenchmark checkout SHA");
    return;
  }
  const master = process.env.FLINT_MASTER === "true";
  const bootstrap = process.env.FLINT_BOOTSTRAP === "true";
  if (bootstrap && !master) {
    core.setFailed("Flint baseline bootstrap must run on master");
    return;
  }

  const errors = curr.filter((test) => test.error);
  const panics = errors.filter((test) => test.error === "Flint test panicked");
  const hasPassing = curr.some((test) => test.success && !test.skipped);
  const formatErrors = (list) => list.map((test) => `${test.name}: ${test.error}`).join("\n");

  const base = readJson(BASELINE);
  if (!base) {
    if (panics.length > 0) {
      core.setFailed(formatErrors(panics));
    } else if (master && !bootstrap) {
      core.setFailed(`Flint baseline unavailable for ${benchmarkSha}. Use an explicit master bootstrap for a new revision.`);
    } else if (bootstrap && !hasPassing) {
      core.setFailed("Cannot bootstrap a baseline without a passing Flint test");
    } else if (errors.length > 0 && !hasPassing) {
      core.setFailed(formatErrors(errors));
    } else {
      if (errors.length > 0) {
        core.warning(`Non-blocking Flint test errors:\n${formatErrors(errors)}`);
      }
      if (!bootstrap) {
        core.warning(`No Flint baseline available for ${benchmarkSha}`);
      }
    }
    return;
  }
  if (base.benchmarkSha !== benchmarkSha || !Array.isArray(base.tests) ||
      !base.tests.some((test) => test.success)) {
    core.setFailed(`Invalid Flint baseline for ${benchmarkSha}`);
    return;
  }
  if (bootstrap) {
    core.setFailed(`A Flint baseline already exists for ${benchmarkSha}`);
    return;
  }

  const regressions = findRegressions(base.tests, curr);
  fs.writeFileSync(REGRESSIONS, JSON.stringify(regressions, null, 2));
  if (panics.length > 0) {
    core.setFailed(formatErrors(panics));
    return;
  }
  if (errors.length > 0 && !hasPassing) {
    core.setFailed(formatErrors(errors));
    return;
  }
  const basePassed = new Set(names(base.tests.filter((test) => test.success)));
  const baselineErrors = errors.filter((test) => basePassed.has(test.name));
  if (baselineErrors.length > 0) {
    core.setFailed(formatErrors(baselineErrors));
    return;
  }
  const newErrors = errors.filter((test) => !basePassed.has(test.name));
  if (newErrors.length > 0) {
    core.warning(`Non-blocking Flint test errors in non-baseline tests:\n${formatErrors(newErrors)}`);
  }
  const missing = regressions.filter((test) => test.missing);
  if (missing.length > 0) {
    core.setFailed(`Flint tests from the pinned baseline did not execute: ${names(missing).join(", ")}`);
  }
}

function checkRegressions({ core }) {
  const regressions = readJson(REGRESSIONS);
  if (!regressions) {
    core.info("No baseline comparison available");
    return;
  }
  if (regressions.length === 0) {
    core.info("No regressions detected");
    return;
  }
  core.info(regressions.map((r) => `  - ${regressionLine(r)}`).join("\n"));
  core.setFailed(`Regressions detected: ${JSON.stringify(names(regressions))}`);
}

async function warnRegressions({ core }) {
  let msg = "Flint regression check failed but is not blocking yet (continue-on-error).";
  const regressions = readJson(REGRESSIONS);
  if (regressions?.length) {
    msg += ` Regressions: ${names(regressions).join(", ")}`;
  }
  core.warning(msg, { title: "Flint regressions" });
  await core.summary
    .addHeading(":warning: Flint regression check failed (non-blocking)", 2)
    .addRaw(msg, true)
    .write();
}

function prReport({ core, context }) {
  const runUrl = `${context.serverUrl}/${context.repo.owner}/${context.repo.repo}/actions/runs/${context.runId}`;
  const body = buildReport({
    summary: readJson(SUMMARY),
    regressions: readJson(REGRESSIONS),
    currentIds: readLines(IMPLEMENTED),
    baseIds: readLines(BASE_IMPLEMENTED),
    runUrl,
  });
  fs.mkdirSync(REPORT_DIR, { recursive: true });
  fs.writeFileSync(`${REPORT_DIR}/pr_number.txt`, `${context.payload.pull_request.number}\n`);
  fs.writeFileSync(`${REPORT_DIR}/comment.md`, `${body}\n`);
  core.info(body);
}

function mergeBaselineStep() {
  const curr = readJson(SUMMARY);
  const base = readJson(BASELINE);
  const executed = curr.filter((test) => !test.skipped);
  const tests = mergeBaseline(base?.tests ?? [], executed);
  const baseline = { benchmarkSha: process.env.FLINT_BENCHMARK_SHA, tests };
  fs.writeFileSync(MERGED_BASELINE, JSON.stringify(baseline, null, 2));
}

module.exports = {
  findRegressions,
  mergeBaseline,
  newIds,
  buildReport,
  validateInputs,
  checkRegressions,
  warnRegressions,
  prReport,
  mergeBaselineStep,
};
