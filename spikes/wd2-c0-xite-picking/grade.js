'use strict';
// WD2-C0 grader. Compares a mapping result with the ORACLE's authored truth.
// Imports neither the mapping nor src/ -- it compares spans and statuses only.

const CATEGORIES = Object.freeze([
  'PROVEN', 'REFUSED_AMBIGUOUS', 'REFUSED_EXTERNAL', 'REFUSED_SENSOR_CONFLICT', 'UNSUPPORTED', 'WRONG',
]);

const sameSpan = (a, b) => !!a && !!b && a.start === b.start && a.end === b.end;

// WRONG iff the mapping claims PROVEN and the source occurrence it names is
// not the one the oracle says was clicked (or the logical object it promotes
// to is not the oracle's). A refusal is never WRONG.
function grade(expect, spans, result) {
  const truthClicked = expect.clicked ? spans[expect.clicked] : null;
  const truthLogical = expect.logical ? spans[expect.logical] : truthClicked;
  let wrong = false;
  let wrongWhy = null;
  if (result.status === 'PROVEN') {
    if (expect.status === 'NO_HIT') { wrong = true; wrongWhy = 'proved-a-node-where-oracle-expects-no-geometry'; }
    else if (!sameSpan(result.source.logical, truthLogical)) { wrong = true; wrongWhy = 'logical-occurrence-differs-from-oracle'; }
    else if (expect.status === 'PROVEN' && !sameSpan(result.source.shape, spans[expect.clicked])) { wrong = true; wrongWhy = 'shape-occurrence-differs-from-oracle'; }
  }
  if (expect.status !== 'NO_HIT' && result.status === 'NO_HIT') {
    // The click missed the geometry it aimed at: a harness/coordinate failure,
    // not a wrong selection -- reported separately.
  }
  const category = wrong ? 'WRONG' : result.status;
  return {
    category,
    wrong,
    wrongWhy,
    matchesExpectation: !wrong && result.status === expect.status,
    truth: { status: expect.status, clicked: expect.clicked || null, clickedSpan: truthClicked, logical: expect.logical || expect.clicked || null, logicalSpan: truthLogical },
  };
}

module.exports = { CATEGORIES, grade, sameSpan };
