import QtQuick
import QtQuick.Templates as T
import qs.Commons
import qs.Ui

import "Api.js" as Api

Item {
  id: root

  property string iconText: ""
  property string tooltipText: ""
  property bool selected: false
  property bool enabled: true
  property bool hasCursor: false
  property color foreground: Color.foreground
  property real iconSize: Style.font.icon
  property real chickletSize: Style.space(32)

  signal clicked()
  signal hovered(bool isHovered)

  readonly property bool hot: mouseArea.containsMouse || root.hasCursor
  readonly property color pillBorder: Style.controlBorder(false,
    root.hot || root.selected, root.foreground, Color.accent)

  implicitWidth: chickletSize
  implicitHeight: chickletSize
  width: chickletSize
  height: chickletSize

  Accessible.role: Accessible.Button
  Accessible.name: root.tooltipText
  Accessible.description: root.tooltipText
  Accessible.onPressAction: if (root.enabled) root.clicked()

  Rectangle {
    anchors.fill: parent
    radius: height / 2
    color: mouseArea.pressed && root.enabled
      ? Style.pressedFillFor(root.foreground, Color.accent)
      : (root.selected
        ? Style.selectedFillFor(root.foreground, Color.accent)
        : (root.hot && root.enabled
          ? Style.hoverFillFor(root.foreground, Color.accent)
          : Qt.rgba(0, 0, 0, 0)))
    border.width: Math.max(1, Style.normalBorderWidth)
    border.color: root.pillBorder
    opacity: root.enabled ? 1 : 0.45

    Behavior on color { ColorAnimation { duration: 120 } }
    Behavior on border.color { ColorAnimation { duration: 120 } }
  }

  Text {
    anchors.centerIn: parent
    text: root.iconText
    color: root.selected
      ? Style.selectedStateColor(root.foreground, Color.accent)
      : root.foreground
    font.family: Style.font.family
    font.pixelSize: root.iconSize
    opacity: root.enabled ? 1 : 0.45
  }

  MouseArea {
    id: mouseArea
    anchors.fill: parent
    hoverEnabled: true
    enabled: root.enabled
    cursorShape: root.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
    onPressed: {
      tooltipShow.stop()
      tooltip.close()
    }
    onClicked: root.clicked()
    onContainsMouseChanged: {
      root.hovered(containsMouse)
      root.syncTooltipHover()
    }
  }

  function syncTooltipHover() {
    if (Api.chickletTooltipOpen(root.tooltipText, mouseArea.containsMouse,
        root.hasCursor)) {
      tooltipHide.stop()
      tooltipShow.restart()
    } else {
      tooltipShow.stop()
      tooltipHide.restart()
    }
  }

  onTooltipTextChanged: {
    if (root.tooltipText === "") {
      tooltipShow.stop()
      tooltip.close()
    }
  }

  Timer {
    id: tooltipShow
    interval: Api.chickletTooltipShowMs()
    onTriggered: {
      if (Api.chickletTooltipOpen(root.tooltipText, mouseArea.containsMouse,
          root.hasCursor))
        tooltip.open()
    }
  }

  Timer {
    id: tooltipHide
    interval: Api.chickletTooltipHideMs()
    onTriggered: {
      if (!mouseArea.containsMouse) tooltip.close()
    }
  }

  PanelToolTip {
    id: tooltip
    text: root.tooltipText
    delay: 0
    // Stay in the panel window. A Wayland popup under the cursor leaves
    // the chicklet, hides the hint, then shows it again — and eats play.
    popupType: T.Popup.Item
    closePolicy: T.Popup.NoAutoClose
    margins: 0
    readonly property var tipPos: Api.chickletTooltipXY(root.width,
      implicitWidth, implicitHeight, Style.space(8))
    x: tipPos.x
    y: tipPos.y
  }
}
