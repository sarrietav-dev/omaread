import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model
import "ServiceHost.js" as ServiceHost

// What you're reading, in the bar: a book you can click back into.
//   omarchy-shell sarrietav-dev.omaread toggle
Panel {
  id: root
  moduleName: "sarrietav-dev.omaread"
  ipcTarget: "sarrietav-dev.omaread"

  property int selectedIndex: 0

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // Stands in until the host has built the real service.
  QtObject {
    id: dummyService
    property var reading: []
    property string lastError: ""
    property bool refreshing: false
    function refresh() {}
    function open() {}
  }

  readonly property var hostedService: ServiceHost.hostedService(bar)
  readonly property var service: hostedService !== null ? hostedService : dummyService
  readonly property var reading: service.reading || []

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color dim: Qt.darker(foreground, 1.6)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  // The service polls on a timer; the widget owns the setting that tunes it.
  readonly property int refreshIntervalSec: {
    var value = Number(setting("refreshIntervalSec", 300))
    return isFinite(value) ? Math.max(60, Math.min(3600, value)) : 300
  }
  onRefreshIntervalSecChanged: if (hostedService) hostedService.refreshIntervalSec = refreshIntervalSec
  onHostedServiceChanged: if (hostedService) hostedService.refreshIntervalSec = refreshIntervalSec

  function moveSelection(delta) {
    if (!reading.length) return
    selectedIndex = Math.max(0, Math.min(reading.length - 1, selectedIndex + delta))
  }

  function openSelected() {
    var book = reading[selectedIndex]
    service.open(book ? book.path : "")
    close()
  }

  onOpenedChanged: if (opened) { selectedIndex = 0; service.refresh() }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    iconComponent: Component {
      Item {
        ReadingIcon {
          anchors.centerIn: parent
          iconSize: Style.space(14)
          color: root.foreground
        }
      }
    }
    tooltipText: Model.tooltip(root.reading)
    onPressed: function (buttonCode) {
      if (buttonCode === Qt.MiddleButton) root.service.open(root.reading.length ? root.reading[0].path : "")
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(360))
    contentHeight: panel.fittedContentHeight(
      Math.min(rows.implicitHeight + Style.space(70), Style.space(560)),
      Style.space(560))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function (dx, dy) { if (dy !== 0) root.moveSelection(dy) }
      onActivateRequested: root.openSelected()
      onCloseRequested: root.close()
      onTabRequested: function (direction) { root.switchPanel(direction) }
      onTextKey: function (text) {
        if (text === "j") root.moveSelection(1)
        else if (text === "k") root.moveSelection(-1)
        else if (text === "l" || text === "o") { root.service.open(""); root.close() }
        else if (text === "r") root.service.refresh()
      }

      ColumnLayout {
        anchors.fill: parent
        spacing: Style.space(8)

        // The title is the way into the library.
        Item {
          Layout.fillWidth: true
          implicitHeight: titleRow.implicitHeight + Style.space(6)

          Rectangle {
            anchors.fill: parent
            anchors.leftMargin: -Style.space(6)
            anchors.rightMargin: -Style.space(6)
            color: root.foreground
            opacity: titleHover.hovered ? Style.hoverFillAlpha : 0
          }
          HoverHandler { id: titleHover }
          TapHandler { onTapped: { root.service.open(""); root.close() } }

          RowLayout {
            id: titleRow
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(7)
            ReadingIcon { iconSize: Style.space(15); color: root.foreground }
            Text {
              text: "Omaread"
              font.family: root.fontFamily
              font.pixelSize: Style.font.title
              font.bold: true
              color: root.foreground
            }
          }
          Text {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            text: "o library"
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            color: root.dim
          }
        }

        Text {
          Layout.fillWidth: true
          visible: root.service.lastError !== ""
          text: root.service.lastError
          wrapMode: Text.WordWrap
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
          color: root.dim
        }

        Text {
          Layout.fillWidth: true
          visible: root.reading.length === 0 && root.service.lastError === ""
          text: "Not reading anything yet."
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
          color: root.dim
        }

        ScrollView {
          Layout.fillWidth: true
          Layout.fillHeight: true
          clip: true
          ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

          ColumnLayout {
            id: rows
            width: Math.max(0, parent.width)
            spacing: Style.space(4)

            Repeater {
              model: root.reading

              delegate: Item {
                Layout.fillWidth: true
                implicitHeight: bookColumn.implicitHeight + Style.space(12)

                Rectangle {
                  anchors.fill: parent
                  anchors.leftMargin: -Style.space(6)
                  anchors.rightMargin: -Style.space(6)
                  color: root.foreground
                  opacity: index === root.selectedIndex ? Style.selectedFillAlpha : (hover.hovered ? Style.hoverFillAlpha : 0)
                }
                HoverHandler {
                  id: hover
                  onHoveredChanged: if (hovered) root.selectedIndex = index
                }
                TapHandler { onTapped: { root.selectedIndex = index; root.openSelected() } }

                ColumnLayout {
                  id: bookColumn
                  anchors.left: parent.left
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  spacing: Style.space(3)

                  Text {
                    Layout.fillWidth: true
                    text: modelData.title || ""
                    elide: Text.ElideRight
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.body
                    font.bold: index === 0
                    color: root.foreground
                  }
                  Text {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: Model.byline(modelData)
                    elide: Text.ElideRight
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                    color: root.dim
                  }
                  // How far along, as a hairline the width of the row.
                  Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: Math.max(2, Style.space(2))
                    color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.15)
                    Rectangle {
                      width: parent.width * Math.max(0, Math.min(1, Number(modelData.fraction) || 0))
                      height: parent.height
                      color: Color.accent
                    }
                  }
                  Text {
                    Layout.fillWidth: true
                    text: Model.detail(modelData)
                    elide: Text.ElideRight
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                    color: root.dim
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
