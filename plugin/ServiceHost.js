// One service for the whole shell, however many bars: this finds it.
function hostedService(bar) {
  var shell = bar ? bar.shell : null
  if (!shell || typeof shell.serviceFor !== "function") return null
  return shell.serviceFor("sarrietav-dev.omaread") || null
}

if (typeof module !== "undefined") {
  module.exports = { hostedService: hostedService }
}
