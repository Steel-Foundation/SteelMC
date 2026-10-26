const test = require("node:test");
const assert = require("node:assert");
const { findRegressions, mergeBaseline, buildReport } = require("./flint-report.js");

const t = (name, success, ids = []) => ({ name, ids, success });

test("regressions: only baseline successes that fail or vanish", () => {
  const base = [t("a", true), t("b", true), t("c", true), t("d", false)];
  const curr = [t("a", true), t("b", false), t("d", false)];
  assert.deepStrictEqual(findRegressions(base, curr), [
    { name: "b", missing: false },
    { name: "c", missing: true },
  ]);
});

test("merge: success is sticky, ids come from the current run", () => {
  const base = [t("a", true, ["old"]), t("b", false)];
  const curr = [t("a", false, ["new"]), t("c", true)];
  assert.deepStrictEqual(mergeBaseline(base, curr), [
    t("a", true, ["new"]),
    t("b", false),
    t("c", true),
  ]);
});

test("report: newly implemented ids list their covering tests", () => {
  const summary = [t("fence", false, ["minecraft:fence"]), t("other", true, ["minecraft:stone"])];
  const body = buildReport({
    summary,
    regressions: [],
    currentIds: ["minecraft:fence", "minecraft:wall", "minecraft:stone"],
    baseIds: ["minecraft:stone"],
    runUrl: "https://example/run",
  });
  assert.match(body, /`minecraft:fence`, `minecraft:wall`\n\n\*\*1 tests\*\*, 0 passed, 1 failed/);
  assert.match(body, /Failed:\n- :x: `fence`/);
  assert.match(body, /None :white_check_mark:/);
});

test("report: missing summary and missing baseline", () => {
  assert.match(buildReport({ summary: null, runUrl: "u" }), /failed to compile/);
  const body = buildReport({ summary: [], regressions: null, currentIds: null, baseIds: null, runUrl: "u" });
  assert.match(body, /No master baseline yet/);
  assert.match(body, /No implemented-id list/);
});
