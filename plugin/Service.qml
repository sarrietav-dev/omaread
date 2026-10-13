import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

// Asks omaread what you're reading. Omaread pings `refresh` when you close a
// book, so the timer is only a backstop.
Item {
  id: root

  property var settings: ({})
  property var reading: []
  property string lastError: ""
  readonly property bool refreshing: statusProcess.running

  readonly property int refreshIntervalSec: {
    var value = parseInt(String(settings && settings.refreshIntervalSec ? settings.refreshIntervalSec : 300), 10)
    return isFinite(value) ? Math.max(60, Math.min(3600, value)) : 300
  }

  property string _output: ""
  property string _error: ""

  function refresh() {
    if (statusProcess.running) return
    statusProcess.running = true
  }

  // Open a book (or the library, with no path) in the running app.
  function open(path) {
    launchProcess.command = path ? ["omaread", path] : ["omaread"]
    launchProcess.running = true
  }

  Component.onCompleted: refresh()

  Timer {
    interval: root.refreshIntervalSec * 1000
    repeat: true
    running: true
    onTriggered: root.refresh()
  }

  IpcHandler {
    target: "sarrietav-dev.omaread-service"
    function refresh(): void { root.refresh() }
  }

  Process {
    id: statusProcess
    running: false
    command: ["omaread", "status", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root._output = text
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root._error = text
    }
    onExited: function (code) {
      if (code !== 0) {
        root.lastError = root._error.trim() || "omaread is not on PATH"
        return
      }
      var parsed = Model.parse(root._output)
      if (parsed.error) {
        root.lastError = parsed.error
        return
      }
      // Kept on failure: a bad refresh shouldn't blank the bar.
      root.reading = parsed.reading
      root.lastError = ""
    }
  }

  Process {
    id: launchProcess
    running: false
    command: []
  }
}
