import QtQuick
import QtQuick.Effects
import qs.Commons
import qs.Ui

Item {
  id: root

  property url source: ""
  property real radius: Math.max(Style.space(14), Style.cornerRadius)
  property color foreground: Color.foreground
  property color accent: Color.accent
  property string altText: "Album artwork"
  property string placeholderText: "󰝚"
  property int sourceSize: 128

  readonly property bool ready: image.status === Image.Ready

  Accessible.role: Accessible.Graphic
  Accessible.name: root.altText

  Rectangle {
    anchors.fill: parent
    radius: root.radius
    color: Style.normalFillFor(root.foreground, root.accent)
  }

  Item {
    id: maskShape
    anchors.fill: parent
    visible: false
    layer.enabled: true
    layer.smooth: true

    Rectangle {
      anchors.fill: parent
      radius: root.radius
      color: "#ffffff"
    }
  }

  Item {
    anchors.fill: parent
    visible: root.ready
    layer.enabled: root.ready
    layer.smooth: true
    layer.effect: MultiEffect {
      maskEnabled: true
      maskSource: maskShape
      maskThresholdMin: 0.5
      maskSpreadAtMin: 0.02
    }

    Image {
      id: image
      anchors.fill: parent
      source: root.source
      sourceSize.width: Math.max(root.sourceSize, Math.round(width * 2))
      sourceSize.height: Math.max(root.sourceSize, Math.round(height * 2))
      fillMode: Image.PreserveAspectCrop
      asynchronous: true
      cache: true
      smooth: true
      mipmap: true
    }
  }

  Text {
    anchors.centerIn: parent
    visible: !root.ready
    text: root.placeholderText
    color: Qt.darker(root.foreground, 1.35)
    font.family: Style.font.family
    font.pixelSize: Style.font.iconLarge
  }

  Rectangle {
    anchors.fill: parent
    radius: root.radius
    color: "transparent"
    border.width: Math.max(1, Style.normalBorderWidth)
    border.color: Style.controlBorder(false, false, root.foreground, root.accent)
  }
}
