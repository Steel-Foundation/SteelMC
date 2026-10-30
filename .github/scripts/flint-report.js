// Data processing for .github/workflows/flint.yml, called through actions/github-script.
const fs = require("fs");

const SUMMARY = "steel-flint/log/flint_summary.json";
const BASELINE = "baseline/flint_baseline.json";
const BASE_IMPLEMENTED = "baseline/implemented.txt";
const IMPLEMENTED = "implemented.txt";
const REGRESSIONS = "regressions.json";
const MERGED_BASELINE = "flint_baseline.json";
const REPORT_DIR = "report";
const MARKER = "<!-- flint-report -->";

const readJson = (path) => (fs.existsSync(path) ? JSON.parse(fs.readFileSync(path, "utf8")) : null);
const readLines = (path) =>
  fs.existsSync(path) ? fs.readFileSync(path, "utf8").split("\n").filter(Boolean) : null;
const sorted = (iterable) => [...iterable].sort();
const names = (entries) => entries.map((e) => e.name);

/** Tests that passed in the baseline but do not pass now (`missing`: absent from the results). */
function findRegressions(base, curr) {
  const present = new Set(names(curr));
  const passed = new Set(names(curr.filter((e) => e.success)));
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
const passedCount = (tests) => tests.filter((t) => t.success).length;

function newlyImplementedSection(summary, currentIds, baseIds) {
  if (!currentIds || !baseIds) return ["No implemented-id list for this run or master yet — skipped."];
  const added = newIds(currentIds, baseIds);
  if (added.length === 0) return ["None."];

  const tests = summary.filter((t) => t.ids.some((id) => added.includes(id)));
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
  const lines = [MARKER, "## Flint test report", ""];
  if (!summary) {
    lines.push(
      `:x: No \`flint_summary.json\` produced — steel-flint likely failed to compile. See the [workflow run](${runUrl}).`,
    );
    return lines.join("\n");
  }

  const failedTotal = summary.length - passedCount(summary);
  lines.push(
    `**Implemented tests:** ${summary.length} total, ${passedCount(summary)} passed, ${failedTotal} failed`,
    "",
    "### Regressions",
  );
  if (!regressions) {
    lines.push("No master baseline yet — skipped.");
  } else if (regressions.length === 0) {
    lines.push("None :white_check_mark:");
  } else {
    lines.push(...regressions.map((r) => `- :x: \`${r.name}\`${r.missing ? " (missing from results)" : ""}`));
  }
  lines.push("", "### Newly implemented blocks/items", ...newlyImplementedSection(summary, currentIds, baseIds));
  lines.push(
    "",
    `<sub>[Workflow run](${runUrl}) · failure links to flint-viz are in the test log</sub>`,
  );
  return lines.join("\n");
}

function checkRegressions({ core }) {
  const curr = readJson(SUMMARY);
  if (!curr) {
    core.setFailed("No flint_summary.json produced — tests likely failed to compile");
    return;
  }
  const base = readJson(BASELINE);
  if (!base) {
    core.info("No baseline found — skipping regression check");
    return;
  }

  const regressions = findRegressions(base, curr);
  fs.writeFileSync(REGRESSIONS, JSON.stringify(regressions, null, 2));
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
  const merged = base ? mergeBaseline(base, curr) : curr;
  fs.writeFileSync(MERGED_BASELINE, JSON.stringify(merged, null, 2));
}

module.exports = {
  findRegressions,
  mergeBaseline,
  newIds,
  buildReport,
  checkRegressions,
  warnRegressions,
  prReport,
  mergeBaselineStep,
};
