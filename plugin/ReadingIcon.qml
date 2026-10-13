import QtQuick

// An open book, drawn rather than loaded so it takes the bar's colour.
Item {
  id: root
  property real iconSize: 14
  property color color: "white"

  implicitWidth: iconSize
  implicitHeight: iconSize
  width: iconSize
  height: iconSize

  readonly property real stroke: Math.max(1.2, iconSize * 0.11)

  // Two pages, each a frame leaning on the spine.
  Repeater {
    model: [0, 1]
    delegate: Item {
      id: pageItem
      anchors.fill: parent
      property int page: modelData
      Rectangle {
        x: modelData === 1 ? root.iconSize * 0.53 : root.iconSize * 0.02
        y: root.iconSize * 0.16
        width: root.iconSize * 0.45
        height: root.iconSize * 0.70
        color: "transparent"
        border.width: root.stroke
        border.color: root.color
      }
      // Lines of text on the page.
      Repeater {
        model: 2
        Rectangle {
          x: (page === 1 ? 0.62 : 0.11) * root.iconSize
          y: (0.36 + index * 0.2) * root.iconSize
          width: root.iconSize * 0.26
          height: Math.max(1, root.stroke * 0.8)
          color: root.color
          opacity: 0.8
        }
      }
    }
  }
}
