// Turning `omaread status --json` into what the panel shows. Plain functions,
// so they can be checked with node (see test.js).

function parse(text) {
  try {
    var data = JSON.parse(text)
    return { reading: Array.isArray(data.reading) ? data.reading : [] }
  } catch (e) {
    return { error: "omaread said something unexpected" }
  }
}

function percent(fraction) {
  var f = Number(fraction)
  if (!isFinite(f)) return "0%"
  return Math.round(Math.max(0, Math.min(1, f)) * 100) + "%"
}

// "12 min", "1 h 05 min", "12 h" — the app's own wording.
function timeLeft(minutes) {
  var m = Number(minutes)
  if (!isFinite(m) || m <= 0) return ""
  if (m < 1) return "<1 min left"
  m = Math.round(m)
  if (m < 60) return m + " min left"
  var h = Math.floor(m / 60), rest = m % 60
  if (h >= 10) return h + " h left"
  return h + " h " + (rest < 10 ? "0" : "") + rest + " min left"
}

function byline(book) {
  return book && Array.isArray(book.authors) ? book.authors.join(", ") : ""
}

// The line under a book's title: "37% · Chapter 4 · 2 h 10 min left".
function detail(book) {
  if (!book) return ""
  var parts = [percent(book.fraction)]
  if (book.chapter) parts.push(book.chapter)
  var left = timeLeft(book.minutesLeft)
  if (left) parts.push(left)
  return parts.join("  ·  ")
}

function tooltip(reading) {
  if (!reading || !reading.length) return "Omaread"
  var b = reading[0]
  return b.title + "  ·  " + percent(b.fraction)
}

if (typeof module !== "undefined") {
  module.exports = { parse: parse, percent: percent, timeLeft: timeLeft, byline: byline, detail: detail, tooltip: tooltip }
}
