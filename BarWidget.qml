import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

import "Api.js" as Api

BarWidget {
  id: root

  moduleName: "wizwam.omamusic"

  readonly property var ytmusic: bar && bar.shell
    ? bar.shell.serviceFor(moduleName) : null
  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property string surfaceKey: "ytmusic-popup-" + String(root)
  readonly property string lyricsRequestKey: surfaceKey + "-lyrics"
  readonly property bool miniPlayerEnabled:
    String(root.setting("showMiniPlayer", "On")) !== "Off"
  property bool popupOpen: false
  property bool lyricsInstallPromptVisible: false
  property bool miniShortcutHelpVisible: false
  property bool popoutSwitchClosing: false
  property bool miniCursorActive: false
  property string miniCursor: "play"
  property real volumeBeforeMute: 0.5
  property string miniSearchText: ""
  readonly property bool opened: popupOpen
  readonly property url iconSource: Qt.resolvedUrl("assets/ytmusic.svg")
  readonly property bool showBarTitle: String(root.setting("showTitle", "On")) !== "Off"
  readonly property bool showBarArtist: String(root.setting("showArtist", "On")) !== "Off"
  readonly property real maxLabelWidth: Style.space(Math.max(120,
    Number(root.setting("maxWidth", 220)) || 220))
  readonly property string barLabel: Api.barTrackText(
    root.ytmusic ? root.ytmusic.title : "",
    root.ytmusic ? root.ytmusic.artist : "",
    root.showBarTitle, root.showBarArtist)
  readonly property bool playing: root.ytmusic && root.ytmusic.playing
  readonly property string barGlyph: {
    if (root.ytmusic && root.ytmusic.hasMedia)
      return root.playing ? "\u{f03e4}" : "\u{f040a}"
    return "󰝚"
  }
  readonly property string barTooltip: {
    if (root.barLabel) return root.barLabel
    if (root.ytmusic && !root.ytmusic.accountConnected) return "Set up YouTube Music"
    return "YouTube Music"
  }
  readonly property color pillFill: Qt.rgba(0, 0, 0, 0)
  readonly property color pillBorder: Style.controlBorder(false, barControlsVisible,
    root.foreground, Color.accent)
  property bool barControlsVisible: false
  readonly property var miniShortcutRows: [
    { keys: "Tab / arrows / HJKL", action: "Select a control" },
    { keys: "Enter", action: "Activate selected button" },
    { keys: "Left / Right", action: "Adjust selected slider" },
    { keys: "Space", action: "Play or pause" },
    { keys: "Ctrl+Left / Right", action: "Previous or next track" },
    { keys: "Shift+Left / Right", action: "Seek 10 seconds" },
    { keys: "Ctrl+Up / Down", action: "Change volume" },
    { keys: "M", action: "Mute or restore volume" },
    { keys: "Ctrl+S / Ctrl+R", action: "Shuffle / repeat" },
    { keys: "Ctrl+Shift+L", action: "Open lyrics" },
    { keys: "E", action: "Cycle EQ preset" },
    { keys: "O", action: "Open full player" },
    { keys: "/  or  Ctrl+K", action: "Search and queue a song" },
    { keys: "Ctrl+/", action: "Toggle this reference" },
    { keys: "Scroll the bar icon", action: "Previous or next track" },
    { keys: "Middle-click the bar icon", action: "Play or pause" },
    { keys: "Esc", action: "Close" }
  ]
  readonly property var miniKeyboardActions: {
    if (lyricsInstallPromptVisible) return ["prompt-cancel", "prompt-confirm"]
    if (miniShortcutHelpVisible) return ["help-close"]
    if (ytmusic && !ytmusic.accountConnected) {
      return ["setup", "search", "open"]
    }
    var actions = []
    if (ytmusic && ytmusic.currentArtistContextAvailable) actions.push("artist")
    if (ytmusic && ytmusic.currentTrackId) actions.push("like")
    if (ytmusic && ytmusic.currentTrackId) actions.push("copy")
    if (ytmusic && ytmusic.currentAlbumItem) actions.push("like-album")
    if (ytmusic && ytmusic.lengthSeconds > 0
        && ytmusic.playbackControllable) actions.push("seek")
    if (ytmusic && ytmusic.playbackControllable) {
      actions.push("shuffle", "previous", "play", "next", "repeat")
    }
    if (ytmusic && ytmusic.lyricsAvailable) actions.push("lyrics")
    if (ytmusic && ytmusic.hasPlayer && ytmusic.volumeSupported)
      actions.push("volume", "eq")
    actions.push("search")
    actions.push("open")
    return actions
  }

  readonly property var miniSearchTracks: {
    var items = ytmusic ? (ytmusic.searchResults || []) : []
    var tracks = []
    for (var i = 0; i < items.length; i++) {
      if (items[i] && items[i].type === "track") tracks.push(items[i])
    }
    return tracks
  }
  readonly property bool miniSearchOpen: String(miniSearchText || "").trim() !== ""
    || (ytmusic && (ytmusic.searchLoading || miniSearchTracks.length > 0))

  function open() { popupOpen = true }
  function close() {
    miniShortcutHelpVisible = false
    popupOpen = false
  }
  function closeForPopoutSwitch() {
    popoutSwitchClosing = true
    close()
    Qt.callLater(function() { root.popoutSwitchClosing = false })
  }
  function toggle() {
    if (miniPlayerEnabled) popupOpen ? close() : open()
    else openFullPanel()
  }

  function shortcutPlayer() {
    return Api.normalizedShortcutPlayer(root.setting("shortcutPlayer",
      "Full player"))
  }

  function toggleMiniPlayerShortcut() {
    if (!bar || typeof bar.isBarWidgetOpen !== "function"
        || typeof bar.hideBarWidget !== "function"
        || typeof bar.summonBarWidget !== "function") return "unavailable"
    if (bar.isBarWidgetOpen(moduleName))
      return bar.hideBarWidget(moduleName) ? "closed" : "unavailable"
    var host = bar.shell
    if (host && typeof host.isPluginOpen === "function"
        && host.isPluginOpen(moduleName) && typeof host.hide === "function") {
      host.hide(moduleName)
      Qt.callLater(function() {
        if (root.bar) root.bar.summonBarWidget(root.moduleName)
      })
      return "opened"
    }
    return bar.summonBarWidget(moduleName) ? "opened" : "unavailable"
  }

  function toggleFullPlayerShortcut() {
    var host = bar ? bar.shell : null
    if (!host || typeof host.isPluginOpen !== "function"
        || typeof host.hide !== "function"
        || typeof host.summon !== "function") return "unavailable"
    if (host.isPluginOpen(moduleName)) {
      host.hide(moduleName)
      return "closed"
    }
    if (bar && typeof bar.isBarWidgetOpen === "function"
        && bar.isBarWidgetOpen(moduleName)
        && typeof bar.hideBarWidget === "function") {
      bar.hideBarWidget(moduleName)
      Qt.callLater(function() {
        if (root.bar && root.bar.shell)
          root.bar.shell.summon(root.moduleName, "{}")
      })
      return "opened"
    }
    return host.summon(moduleName, "{}") ? "opened" : "unavailable"
  }

  function toggleConfiguredPlayerShortcut() {
    var target = shortcutPlayer()
    if (target === "Full player") return toggleFullPlayerShortcut()
    if (target === "Mini player") return toggleMiniPlayerShortcut()
    if (!bar || typeof bar.run !== "function") return "unavailable"
    bar.run("omarchy launch youtube-music || xdg-open https://music.youtube.com")
    return "launched"
  }

  function syncBarControlsVisibility() {
    if (root.vertical) {
      barControlsHide.stop()
      barControlsVisible = false
      return
    }
    if (barHover.hovered || barPreviousControl.tooltipHovered
        || barPlayControl.tooltipHovered || barNextControl.tooltipHovered) {
      barControlsHide.stop()
      barControlsVisible = true
    } else if (barControlsVisible) {
      barControlsHide.restart()
    }
  }

  function controlBarPlayer(mouseButton) {
    if (mouseButton === Qt.RightButton) {
      if (root.ytmusic) root.ytmusic.next()
    } else if (mouseButton === Qt.MiddleButton) {
      if (root.ytmusic) root.ytmusic.previous()
    } else {
      root.toggle()
    }
  }

  function prioritizeBarControls() {
    if (!bar || !bar.registerClickTarget || !bar.unregisterClickTarget) return
    var controls = [barPreviousControl, barPlayControl, barNextControl,
      barVerticalPreviousControl, barVerticalPlayControl, barVerticalNextControl]
    for (var i = 0; i < controls.length; i++) bar.unregisterClickTarget(controls[i])
    for (var j = 0; j < controls.length; j++) bar.registerClickTarget(controls[j])
  }

  function likeCurrentTrack() {
    if (!ytmusic) return
    if (!ytmusic.accountConnected) {
      ytmusic.login()
      openFullPanel({ tab: "login" })
      return
    }
    ytmusic.toggleCurrentTrackSaved()
  }

  function likeCurrentAlbum() {
    if (!ytmusic) return
    if (!ytmusic.accountConnected) {
      ytmusic.login()
      openFullPanel({ tab: "login" })
      return
    }
    ytmusic.toggleCurrentAlbumSaved()
  }

  function openSignInOrPlayer() {
    if (ytmusic && (!ytmusic.accountConnected || Api.isSignInError(ytmusic.lastError)))
      openFullPanel({ tab: "login" })
    else openFullPanel()
  }

  function openFullPanel(payload) {
    close()
    if (!bar || !bar.shell) return
    var encoded = JSON.stringify(payload || ({}))
    if (typeof bar.shell.hide === "function"
        && typeof bar.shell.summon === "function") {
      bar.shell.hide(moduleName)
      Qt.callLater(function() {
        if (root.bar && root.bar.shell)
          root.bar.shell.summon(root.moduleName, encoded)
      })
    } else if (payload && typeof bar.shell.summon === "function")
      bar.shell.summon(moduleName, encoded)
    else bar.shell.toggle(moduleName, encoded)
  }

  IpcHandler {
    target: root.moduleName + ".player"

    function configuredPlayer(): string {
      return root.shortcutPlayer()
    }
    function togglePlayer(): string {
      return root.toggleConfiguredPlayerShortcut()
    }
    function toggleMiniPlayer(): string {
      return root.toggleMiniPlayerShortcut()
    }
    function toggleFullPlayer(): string {
      return root.toggleFullPlayerShortcut()
    }
  }

  function openCurrentArtist() {
    if (!ytmusic || !ytmusic.currentArtistContextAvailable) return
    ytmusic.currentContext("artist", function(item) {
      if (item) root.openFullPanel({ tab: "detail", detailItem: item })
    })
  }

  function openLyrics() {
    if (!ytmusic || !ytmusic.currentLyricsSong) return
    var result = ytmusic.requestLyrics(lyricsRequestKey)
    if (result !== "opening") {
      lyricsInstallPromptVisible = true
      popupOpen = true
    }
  }

  function dismissLyricsInstallPrompt() {
    if (ytmusic) ytmusic.cancelLyricsPlugin(lyricsRequestKey)
    lyricsInstallPromptVisible = false
  }

  function toggleMiniShortcutHelp() {
    if (lyricsInstallPromptVisible) return
    miniShortcutHelpVisible = !miniShortcutHelpVisible
    if (miniShortcutHelpVisible) setMiniCursor("help-close")
    else ensureMiniCursor()
  }

  function ensureMiniCursor() {
    var actions = miniKeyboardActions
    if (!actions.length) {
      miniCursorActive = false
      return
    }
    if (actions.indexOf(miniCursor) >= 0) return
    miniCursor = actions.indexOf("play") >= 0 ? "play" : actions[0]
  }

  function setMiniCursor(action) {
    if (miniKeyboardActions.indexOf(action) < 0) return
    miniCursor = action
    miniCursorActive = true
  }

  function moveMiniCursor(delta) {
    var actions = miniKeyboardActions
    if (!actions.length) return
    var index = actions.indexOf(miniCursor)
    if (index < 0) index = actions.indexOf("play")
    if (index < 0) index = 0
    index = (index + (delta < 0 ? -1 : 1) + actions.length) % actions.length
    miniCursor = actions[index]
    miniCursorActive = true
  }

  function seekBy(seconds) {
    if (!ytmusic || !ytmusic.playbackControllable) return
    ytmusic.seekSeconds(Api.seekPosition(ytmusic.positionSeconds, seconds,
      ytmusic.lengthSeconds))
  }

  function adjustVolume(delta) {
    if (!ytmusic || !ytmusic.volumeSupported) return
    var next = Api.nextVolume(ytmusic.volume, delta)
    if (Api.shouldRememberVolume(next)) volumeBeforeMute = next
    ytmusic.setVolume(next)
  }

  function toggleMute() {
    if (!ytmusic || !ytmusic.volumeSupported) return
    var current = Api.nextVolume(ytmusic.volume, 0)
    if (Api.shouldRememberVolume(current)) {
      volumeBeforeMute = current
      ytmusic.setVolume(0)
    } else ytmusic.setVolume(Api.unmuteVolume(volumeBeforeMute))
  }

  function activateMiniAction(action) {
    if (action === "help-close") toggleMiniShortcutHelp()
    else if (action === "prompt-cancel") dismissLyricsInstallPrompt()
    else if (action === "prompt-confirm") {
      if (ytmusic && !ytmusic.lyricsPluginBusy)
        ytmusic.confirmLyricsPlugin(lyricsRequestKey)
    } else if (action === "artist") openCurrentArtist()
    else if (action === "like") {
      root.likeCurrentTrack()
    } else if (action === "copy") {
      if (ytmusic) ytmusic.copyTrackLink()
    } else if (action === "like-album") {
      root.likeCurrentAlbum()
    } else if (action === "shuffle") {
      if (ytmusic) ytmusic.setShuffle(!ytmusic.shuffle)
    } else if (action === "previous") {
      if (ytmusic) ytmusic.previous()
    } else if (action === "play") {
      if (ytmusic) ytmusic.togglePlayback()
    } else if (action === "next") {
      if (ytmusic) ytmusic.next()
    } else if (action === "repeat") {
      if (ytmusic) ytmusic.cycleRepeat()
    } else if (action === "lyrics") openLyrics()
    else if (action === "volume") toggleMute()
    else if (action === "eq") {
      if (ytmusic) ytmusic.cycleEqPreset()
    } else if (action === "setup") {
      if (ytmusic) ytmusic.login()
      openFullPanel({ tab: "login" })
    } else if (action === "search") focusMiniSearch()
    else if (action === "open") openSignInOrPlayer()
  }

  function focusMiniSearch() {
    if (lyricsInstallPromptVisible || miniShortcutHelpVisible) return
    popupOpen = true
    miniSearchField.forceActiveFocus()
    miniSearchField.selectAll()
  }

  function noteMiniSearch(value) {
    var text = String(value || "")
    if (text === miniSearchText) return
    miniSearchText = text
    if (String(text).trim() === "") {
      miniSearchDebounce.stop()
      if (ytmusic) ytmusic.clearSearch()
      return
    }
    miniSearchDebounce.restart()
  }

  function runMiniSearch() {
    miniSearchDebounce.stop()
    if (ytmusic) ytmusic.search(miniSearchText)
  }

  function clearMiniSearch() {
    miniSearchText = ""
    if (miniSearchField.text !== "") miniSearchField.text = ""
    if (ytmusic) ytmusic.clearSearch()
  }

  function queueMiniSearchItem(item) {
    if (ytmusic) ytmusic.queueItem(item)
  }

  function handleMiniKey(event) {
    var ctrl = (event.modifiers & Qt.ControlModifier) !== 0
    var shift = (event.modifiers & Qt.ShiftModifier) !== 0
    var alt = (event.modifiers & Qt.AltModifier) !== 0
    var plain = !ctrl && !shift && !alt
    var text = String(event.text || "").toLowerCase()

    if (lyricsInstallPromptVisible) {
      if (event.key === Qt.Key_Escape) dismissLyricsInstallPrompt()
      else if (event.key === Qt.Key_Tab || event.key === Qt.Key_Backtab)
        moveMiniCursor(shift || event.key === Qt.Key_Backtab ? -1 : 1)
      else if (event.key === Qt.Key_Left || event.key === Qt.Key_Up
          || text === "h" || text === "k") moveMiniCursor(-1)
      else if (event.key === Qt.Key_Right || event.key === Qt.Key_Down
          || text === "l" || text === "j") moveMiniCursor(1)
      else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter
          || event.key === Qt.Key_Space) {
        if (!event.isAutoRepeat) activateMiniAction(miniCursor)
      } else return
      event.accepted = true
      return
    }

    if (miniShortcutHelpVisible) {
      if (event.key === Qt.Key_Escape
          || event.key === Qt.Key_Return || event.key === Qt.Key_Enter
          || event.key === Qt.Key_Space) {
        if (!event.isAutoRepeat) toggleMiniShortcutHelp()
        event.accepted = true
      }
      return
    }

    if (miniSearchField.activeFocus) {
      if (event.key === Qt.Key_Escape) {
        if (String(miniSearchText || "").trim() !== "") clearMiniSearch()
        else miniKeyCatcher.forceActiveFocus()
        event.accepted = true
      } else if (event.key === Qt.Key_Down) {
        miniKeyCatcher.forceActiveFocus()
        setMiniCursor("search")
        event.accepted = true
      }
      return
    }

    if ((plain && event.key === Qt.Key_Slash)
        || (ctrl && !shift && event.key === Qt.Key_K)) {
      if (!event.isAutoRepeat) focusMiniSearch()
      event.accepted = true
      return
    }

    if (event.key === Qt.Key_Escape) close()
    else if (ctrl && event.key === Qt.Key_Left) { if (ytmusic) ytmusic.previous() }
    else if (ctrl && event.key === Qt.Key_Right) { if (ytmusic) ytmusic.next() }
    else if (shift && !ctrl && event.key === Qt.Key_Left) seekBy(-10)
    else if (shift && !ctrl && event.key === Qt.Key_Right) seekBy(10)
    else if (ctrl && event.key === Qt.Key_Up) adjustVolume(0.05)
    else if (ctrl && event.key === Qt.Key_Down) adjustVolume(-0.05)
    else if (ctrl && !shift && event.key === Qt.Key_S) {
      if (ytmusic && !event.isAutoRepeat) ytmusic.setShuffle(!ytmusic.shuffle)
    } else if (ctrl && !shift && event.key === Qt.Key_R) {
      if (ytmusic && !event.isAutoRepeat) ytmusic.cycleRepeat()
    } else if (ctrl && shift && event.key === Qt.Key_L) {
      if (!event.isAutoRepeat) openLyrics()
    } else if (plain && event.key === Qt.Key_Space) {
      if (ytmusic && !event.isAutoRepeat) ytmusic.togglePlayback()
    } else if (plain && event.key === Qt.Key_M) {
      if (!event.isAutoRepeat) toggleMute()
    } else if (plain && event.key === Qt.Key_O) {
      if (!event.isAutoRepeat) openSignInOrPlayer()
    } else if (plain && event.key === Qt.Key_E) {
      if (!event.isAutoRepeat && ytmusic) ytmusic.cycleEqPreset()
    } else if (event.key === Qt.Key_Tab || event.key === Qt.Key_Backtab) {
      moveMiniCursor(shift || event.key === Qt.Key_Backtab ? -1 : 1)
    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
      if (!event.isAutoRepeat) activateMiniAction(miniCursor)
    } else if (plain && (event.key === Qt.Key_Left || text === "h")) {
      if (miniCursor === "seek") seekBy(-5)
      else if (miniCursor === "volume") adjustVolume(-0.05)
      else moveMiniCursor(-1)
    } else if (plain && (event.key === Qt.Key_Right || text === "l")) {
      if (miniCursor === "seek") seekBy(5)
      else if (miniCursor === "volume") adjustVolume(0.05)
      else moveMiniCursor(1)
    } else if (plain && (event.key === Qt.Key_Up || text === "k")) moveMiniCursor(-1)
    else if (plain && (event.key === Qt.Key_Down || text === "j")) moveMiniCursor(1)
    else if (plain && event.key === Qt.Key_Home) setMiniCursor(miniKeyboardActions[0])
    else if (plain && event.key === Qt.Key_End)
      setMiniCursor(miniKeyboardActions[miniKeyboardActions.length - 1])
    else return
    event.accepted = true
  }

  function syncSettings() {
    if (ytmusic) ytmusic.applySettings(settings)
  }

  implicitWidth: root.vertical ? root.barSize : barContent.implicitWidth + Style.space(18)
  implicitHeight: root.vertical ? barVerticalControls.implicitHeight : root.barSize

  Behavior on implicitWidth { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }

  onSettingsChanged: syncSettings()
  onYtmusicChanged: syncSettings()
  onMiniPlayerEnabledChanged: if (!miniPlayerEnabled) close()
  onMiniKeyboardActionsChanged: ensureMiniCursor()
  onBarLabelChanged: barMarqueeStart.restart()
  onVerticalChanged: syncBarControlsVisibility()
  onVisibleChanged: if (!visible) barControlsVisible = false
  onLyricsInstallPromptVisibleChanged: {
    if (lyricsInstallPromptVisible) {
      miniCursor = "prompt-cancel"
      miniCursorActive = true
    } else ensureMiniCursor()
  }
  onPopupOpenChanged: {
    if (popupOpen) {
      miniCursor = miniKeyboardActions.indexOf("play") >= 0
        ? "play" : miniKeyboardActions[0]
      miniCursorActive = miniKeyboardActions.length > 0
    } else miniCursorActive = false
    if (ytmusic) ytmusic.setUiVisible(surfaceKey, popupOpen)
    if (!popupOpen && lyricsInstallPromptVisible
        && (!ytmusic || !ytmusic.lyricsPluginBusy)) {
      if (ytmusic) ytmusic.cancelLyricsPlugin(lyricsRequestKey)
      lyricsInstallPromptVisible = false
    }
    if (!popupOpen) clearMiniSearch()
  }

  Timer {
    id: miniSearchDebounce
    interval: 200
    repeat: false
    onTriggered: root.runMiniSearch()
  }
  Timer {
    id: barControlsHide
    interval: 180
    onTriggered: root.barControlsVisible = false
  }
  Timer {
    id: barMarqueeStart
    interval: 180
    onTriggered: {
      barLabelText.x = 0
      if (barLabelClip.overflow > 0) barMarquee.restart()
    }
  }
  HoverHandler {
    id: barHover
    onHoveredChanged: root.syncBarControlsVisibility()
  }
  Component.onCompleted: {
    syncSettings()
    Qt.callLater(function() { root.prioritizeBarControls() })
  }
  Component.onDestruction: if (ytmusic) ytmusic.setUiVisible(surfaceKey, false)

  WidgetButton {
    id: button
    anchors.fill: parent
    clip: true
    bar: root.bar
    text: " "
    labelVisible: false
    hasVisualContent: true
    keepSpace: true
    dimmed: !root.vertical && !root.playing
    tooltipText: root.vertical ? "" : root.barTooltip
    fixedWidth: root.vertical ? root.barSize : -1
    fixedHeight: root.vertical ? Style.bar.statusSlot : -1
    Accessible.role: Accessible.Button
    Accessible.name: root.barTooltip
    Accessible.description: root.playing
      ? "Playing. Left click opens the player. Right click skips forward."
      : "Left click opens the player. Hover for previous, play, and next."
    onPressed: function(mouseButton) { root.controlBarPlayer(mouseButton) }
    onWheelMoved: function(delta) {
      if (!root.ytmusic) return
      if (delta > 0) root.ytmusic.previous()
      else if (delta < 0) root.ytmusic.next()
    }

    Rectangle {
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.top: parent.top
      anchors.bottom: parent.bottom
      anchors.topMargin: 2
      anchors.bottomMargin: 2
      z: -1
      radius: height / 2
      color: root.pillFill
      border.width: Style.controlBorderWidth(false, root.barControlsVisible)
      border.color: root.pillBorder
      visible: !root.vertical

      Behavior on color { ColorAnimation { duration: 120 } }
      Behavior on border.color { ColorAnimation { duration: 120 } }
    }

    Column {
      id: barVerticalControls
      anchors.centerIn: parent
      visible: root.vertical
      spacing: Style.space(2)

      WidgetButton {
        id: barVerticalPreviousControl
        bar: root.bar
        text: "\u{f04ae}"
        fontSize: Style.font.bodySmall
        foreground: root.foreground
        fixedWidth: root.barSize
        fixedHeight: Style.space(24)
        dimmed: !root.ytmusic || !root.ytmusic.playbackControllable
        pressable: root.ytmusic && root.ytmusic.playbackControllable
        tooltipText: Api.controlTooltip("Previous track", "Ctrl+Left")
        Accessible.name: "Previous track"
        onPressed: function(mouseButton) {
          if (mouseButton === Qt.LeftButton && root.ytmusic) root.ytmusic.previous()
        }
      }

      WidgetButton {
        id: barVerticalPlayControl
        bar: root.bar
        text: root.barGlyph
        fontSize: Style.font.bodySmall
        foreground: root.foreground
        fixedWidth: root.barSize
        fixedHeight: Style.space(24)
        tooltipText: root.barTooltip
        Accessible.name: root.playing ? "Pause" : "Play"
        onPressed: function(mouseButton) { root.controlBarPlayer(mouseButton) }
      }

      WidgetButton {
        id: barVerticalNextControl
        bar: root.bar
        text: "\u{f04ad}"
        fontSize: Style.font.bodySmall
        foreground: root.foreground
        fixedWidth: root.barSize
        fixedHeight: Style.space(24)
        dimmed: !root.ytmusic || !root.ytmusic.playbackControllable
        pressable: root.ytmusic && root.ytmusic.playbackControllable
        tooltipText: Api.controlTooltip("Next track", "Ctrl+Right")
        Accessible.name: "Next track"
        onPressed: function(mouseButton) {
          if (mouseButton === Qt.LeftButton && root.ytmusic) root.ytmusic.next()
        }
      }
    }

    Item {
      id: barContent
      anchors.centerIn: parent
      visible: !root.vertical
      readonly property real transportGap: Style.space(6)
      readonly property real transportWidth: barPreviousControl.width + transportGap
        + barPlayControl.width + transportGap + barNextControl.width
      implicitWidth: Math.max(Style.space(78),
        transportWidth + transportGap + root.maxLabelWidth)
      width: implicitWidth
      height: parent.height

      WidgetButton {
        id: barPreviousControl
        bar: root.bar
        text: "\u{f04ae}"
        fontSize: Style.font.bodySmall
        foreground: root.foreground
        fixedWidth: Style.space(20)
        fixedHeight: root.barSize
        visible: !root.vertical
        interactive: root.barControlsVisible
        opacity: root.barControlsVisible
          ? (root.ytmusic && root.ytmusic.playbackControllable ? 1 : 0.45) : 0
        pressable: root.ytmusic && root.ytmusic.playbackControllable
        tooltipText: Api.controlTooltip("Previous track", "Ctrl+Left")
        Accessible.name: "Previous track"
        x: 0
        z: 1
        anchors.verticalCenter: parent.verticalCenter
        onTooltipHoveredChanged: root.syncBarControlsVisibility()
        onPressed: function(mouseButton) {
          if (mouseButton === Qt.LeftButton && root.ytmusic) root.ytmusic.previous()
        }
      }

      Text {
        id: barGlyphText
        anchors.verticalCenter: parent.verticalCenter
        opacity: root.barControlsVisible ? 0 : 1
        x: barPlayControl.x
        width: barPlayControl.width
        horizontalAlignment: Text.AlignHCenter
        text: root.barGlyph
        color: root.foreground
        font.family: root.bar ? root.bar.fontFamily : Style.font.family
        font.pixelSize: Style.font.bodySmall
        Accessible.ignored: true

        Behavior on opacity { OpacityAnimator { duration: 120; easing.type: Easing.OutCubic } }
      }

      WidgetButton {
        id: barPlayControl
        bar: root.bar
        text: root.barGlyph
        fontSize: Style.font.bodySmall
        foreground: root.foreground
        fixedWidth: Style.space(20)
        fixedHeight: root.barSize
        visible: !root.vertical
        interactive: root.barControlsVisible
        opacity: root.barControlsVisible ? 1 : 0
        tooltipText: Api.controlTooltip(root.playing ? "Pause" : "Play", "Space")
        Accessible.name: root.playing ? "Pause" : "Play"
        x: barPreviousControl.width + parent.transportGap
        z: 1
        anchors.verticalCenter: parent.verticalCenter
        onTooltipHoveredChanged: root.syncBarControlsVisibility()
        onPressed: function(mouseButton) {
          if (mouseButton === Qt.LeftButton && root.ytmusic)
            root.ytmusic.togglePlayback()
          else root.controlBarPlayer(mouseButton)
        }
      }

      WidgetButton {
        id: barNextControl
        bar: root.bar
        text: "\u{f04ad}"
        fontSize: Style.font.bodySmall
        foreground: root.foreground
        fixedWidth: Style.space(20)
        fixedHeight: root.barSize
        visible: !root.vertical
        interactive: root.barControlsVisible
        opacity: root.barControlsVisible
          ? (root.ytmusic && root.ytmusic.playbackControllable ? 1 : 0.45) : 0
        pressable: root.ytmusic && root.ytmusic.playbackControllable
        tooltipText: Api.controlTooltip("Next track", "Ctrl+Right")
        Accessible.name: "Next track"
        x: barPlayControl.x + barPlayControl.width + parent.transportGap
        z: 1
        anchors.verticalCenter: parent.verticalCenter
        onTooltipHoveredChanged: root.syncBarControlsVisibility()
        onPressed: function(mouseButton) {
          if (mouseButton === Qt.LeftButton && root.ytmusic) root.ytmusic.next()
        }
      }

      Item {
        id: barLabelClip
        readonly property real normalWidth: root.maxLabelWidth
        x: barNextControl.x + barNextControl.width + parent.transportGap
        width: normalWidth
        height: barGlyphText.implicitHeight
        clip: true
        anchors.verticalCenter: parent.verticalCenter
        readonly property real overflow: Math.max(0, barLabelText.implicitWidth - width)

        Text {
          id: barLabelText
          anchors.verticalCenter: parent.verticalCenter
          text: root.barLabel || "YouTube Music"
          color: root.foreground
          font.family: root.bar ? root.bar.fontFamily : Style.font.family
          font.pixelSize: Style.font.bodySmall
          renderType: Text.NativeRendering
          Accessible.ignored: true
        }

        SequentialAnimation {
          id: barMarquee
          PauseAnimation { duration: 650 }
          NumberAnimation {
            target: barLabelText
            property: "x"
            from: 0
            to: -barLabelClip.overflow
            duration: Math.max(1600, barLabelClip.overflow * 24)
            easing.type: Easing.InOutSine
          }
          PauseAnimation { duration: 900 }
          NumberAnimation {
            target: barLabelText
            property: "x"
            from: -barLabelClip.overflow
            to: 0
            duration: Math.max(1200, barLabelClip.overflow * 18)
            easing.type: Easing.InOutSine
          }
        }

        Rectangle {
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          width: Math.min(Style.space(24), parent.width)
          height: parent.height
          visible: parent.width > 0
          gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: Qt.rgba(root.pillFill.r, root.pillFill.g, root.pillFill.b, 0) }
            GradientStop { position: 1; color: root.pillFill }
          }
        }
      }
    }
  }

  KeyboardPanel {
    id: popup
    anchorItem: button
    bar: root.bar
    owner: root
    open: root.popupOpen
    focusTarget: miniKeyCatcher
    contentWidth: fittedContentWidth(Style.space(360))
    contentHeight: fittedContentHeight(root.miniShortcutHelpVisible
      ? miniShortcutHelp.implicitHeight : contentColumn.implicitHeight)

    Item {
      id: miniKeyCatcher
      anchors.fill: parent
      focus: true
      Keys.priority: Keys.BeforeItem
      Keys.onPressed: function(event) { root.handleMiniKey(event) }

      Shortcut {
        sequence: "Ctrl+/"
        enabled: root.popupOpen && !root.lyricsInstallPromptVisible
        onActivated: root.toggleMiniShortcutHelp()
      }

      Column {
        id: contentColumn
        anchors.fill: parent
        visible: !root.miniShortcutHelpVisible
        spacing: Style.space(10)

        Column {
          width: parent.width
          spacing: Style.space(8)
          visible: !root.lyricsInstallPromptVisible
            && root.ytmusic && !root.ytmusic.accountConnected

          Text {
            width: parent.width
            text: root.ytmusic ? root.ytmusic.loginProgress : "YouTube Music is unavailable"
            color: root.foreground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.body
            font.bold: true
            wrapMode: Text.WordWrap
          }

          Text {
            width: parent.width
            text: "Connect your YouTube Music account from the full player. Public home shelves work without signing in."
            color: Qt.darker(root.foreground, 1.4)
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.caption
            wrapMode: Text.WordWrap
          }

          Row {
            width: parent.width
            spacing: Style.space(6)

            Button {
              text: "Set up and continue"
              iconText: "󰍂"
              foreground: root.foreground
              hasCursor: root.miniCursorActive && root.miniCursor === "setup"
              onClicked: root.activateMiniAction("setup")
              onHovered: function(on) { if (on) root.setMiniCursor("setup") }
            }
          }
        }

        Column {
          width: parent.width
          spacing: Style.space(8)
          visible: !root.lyricsInstallPromptVisible
            && (!root.ytmusic || root.ytmusic.accountConnected || root.ytmusic.hasMedia)

          Row {
            spacing: Style.space(2)
            Chicklet {
              id: barCurrentTrackLikeButton
              visible: root.ytmusic && !!root.ytmusic.currentTrackItem
              iconText: root.ytmusic && root.ytmusic.currentTrackSaved
                ? "󰋑" : "󰋕"
              iconSize: Style.font.body
              foreground: root.foreground
              selected: root.ytmusic && root.ytmusic.currentTrackSaved
              hasCursor: root.miniCursorActive && root.miniCursor === "like"
              enabled: root.ytmusic && !!root.ytmusic.currentTrackId
              chickletSize: Style.space(30)
              tooltipText: root.ytmusic && !root.ytmusic.accountConnected
                ? "Sign in to like"
                : (root.ytmusic && root.ytmusic.currentTrackSaved
                  ? "Remove like" : "Like this song")
              onClicked: root.likeCurrentTrack()
              onHovered: function(on) { if (on) root.setMiniCursor("like") }
            }
            Chicklet {
              visible: root.ytmusic && !!root.ytmusic.currentTrackId
              iconText: "󰆏"
              foreground: root.foreground
              hasCursor: root.miniCursorActive && root.miniCursor === "copy"
              chickletSize: Style.space(30)
              tooltipText: "Copy song link"
              onClicked: if (root.ytmusic) root.ytmusic.copyTrackLink()
              onHovered: function(on) { if (on) root.setMiniCursor("copy") }
            }
            Chicklet {
              visible: root.ytmusic && !!root.ytmusic.currentAlbumItem
              iconText: "󰀥"
              foreground: root.foreground
              selected: root.ytmusic && root.ytmusic.currentAlbumSaved
              hasCursor: root.miniCursorActive && root.miniCursor === "like-album"
              chickletSize: Style.space(30)
              tooltipText: root.ytmusic && !root.ytmusic.accountConnected
                ? "Sign in to like albums"
                : (root.ytmusic && root.ytmusic.currentAlbumSaved
                  ? "Remove album like" : "Like this album")
              onClicked: root.likeCurrentAlbum()
              onHovered: function(on) { if (on) root.setMiniCursor("like-album") }
            }
            Chicklet {
              visible: root.ytmusic && Api.albumShareUrl(root.ytmusic.currentAlbumItem) !== ""
              iconText: "󰌹"
              foreground: root.foreground
              hasCursor: root.miniCursorActive && root.miniCursor === "copy-album"
              chickletSize: Style.space(30)
              tooltipText: "Copy album link"
              onClicked: if (root.ytmusic) root.ytmusic.copyAlbumLink()
              onHovered: function(on) { if (on) root.setMiniCursor("copy-album") }
            }
          }

          Artwork {
            width: Style.space(96)
            height: width
            radius: Math.max(Style.space(14), Style.cornerRadius)
            foreground: root.foreground
            accent: Color.accent
            source: root.popupOpen && root.ytmusic ? root.ytmusic.artUrl : ""
            sourceSize: 192
            altText: Api.artworkAltText(
              root.ytmusic ? root.ytmusic.title : "",
              root.ytmusic ? root.ytmusic.artist : "")
          }

          Column {
            width: parent.width
            spacing: Style.space(2)

            Text {
              width: parent.width
              text: root.ytmusic && root.ytmusic.title
                ? root.ytmusic.title : "Nothing playing"
              color: root.foreground
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.subtitle
              font.bold: true
              wrapMode: Text.Wrap
              maximumLineCount: 2
              elide: Text.ElideRight
              horizontalAlignment: Text.AlignLeft
            }

            Text {
              width: parent.width
              text: root.ytmusic ? root.ytmusic.artist : ""
              color: Qt.darker(root.foreground, 1.4)
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.bodySmall
              wrapMode: Text.Wrap
              maximumLineCount: 2
              elide: Text.ElideRight
              visible: text !== ""
              horizontalAlignment: Text.AlignLeft
            }

            Text {
              width: parent.width
              text: root.ytmusic ? root.ytmusic.album : ""
              color: Color.accent
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.bodySmall
              wrapMode: Text.Wrap
              maximumLineCount: 2
              elide: Text.ElideRight
              visible: text !== ""
              horizontalAlignment: Text.AlignLeft
            }
          }
        }

        Column {
          width: parent.width
          spacing: Style.space(3)
          visible: !root.lyricsInstallPromptVisible
            && root.ytmusic && root.ytmusic.lengthSeconds > 0

          PlaybackSlider {
            id: miniSeekSlider
            width: parent.width
            bar: root.bar
            trackHeight: Style.space(12)
            knobSize: Style.space(18)
            minimum: 0
            maximum: Math.max(1, root.ytmusic ? root.ytmusic.lengthSeconds : 1)
            sourceValue: root.ytmusic ? root.ytmusic.positionSeconds : 0
            sourcePending: root.ytmusic && root.ytmusic.pendingSeek !== null
            acknowledgeTolerance: 2
            contextKey: root.ytmusic ? root.ytmusic.currentUri : ""
            step: 5
            onCommitted: function(value) {
              if (root.ytmusic) root.ytmusic.seekSeconds(value)
            }
          }

          Row {
            width: parent.width
            Text {
              id: positionTime
              text: Api.millisecondsToClock((root.ytmusic ? root.ytmusic.positionSeconds : 0) * 1000)
              color: Qt.darker(root.foreground, 1.45)
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.caption
            }
            Item { width: Math.max(0, parent.width - positionTime.implicitWidth - endTime.implicitWidth); height: 1 }
            Text {
              id: endTime
              text: Api.millisecondsToClock((root.ytmusic ? root.ytmusic.lengthSeconds : 0) * 1000)
              color: Qt.darker(root.foreground, 1.45)
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.caption
            }
          }
        }

        Item {
          width: parent.width
          height: Style.space(38)
          visible: !root.lyricsInstallPromptVisible

          Row {
            id: miniTransport
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(5)

          Chicklet {
            iconText: "󰒟"
            foreground: root.foreground
            selected: root.ytmusic && root.ytmusic.shuffle
            hasCursor: root.miniCursorActive && root.miniCursor === "shuffle"
            tooltipText: "Shuffle · Ctrl+S"
            enabled: root.ytmusic && root.ytmusic.playbackControllable
            onClicked: if (root.ytmusic) root.ytmusic.setShuffle(!root.ytmusic.shuffle)
            onHovered: function(on) { if (on) root.setMiniCursor("shuffle") }
          }
          Chicklet {
            iconText: "󰒮"
            foreground: root.foreground
            hasCursor: root.miniCursorActive && root.miniCursor === "previous"
            tooltipText: "Previous · Ctrl+Left"
            enabled: root.ytmusic && root.ytmusic.playbackControllable
            onClicked: if (root.ytmusic) root.ytmusic.previous()
            onHovered: function(on) { if (on) root.setMiniCursor("previous") }
          }
          Chicklet {
            iconText: root.ytmusic && root.ytmusic.playing ? "󰏤" : "󰐊"
            iconSize: Style.font.iconLarge
            chickletSize: Style.space(38)
            foreground: root.foreground
            hasCursor: root.miniCursorActive && root.miniCursor === "play"
            tooltipText: (root.ytmusic && root.ytmusic.playing ? "Pause" : "Play") + " · Space"
            enabled: root.ytmusic && (root.ytmusic.playbackControllable || root.ytmusic.hasMedia)
            onClicked: if (root.ytmusic) root.ytmusic.togglePlayback()
            onHovered: function(on) { if (on) root.setMiniCursor("play") }
          }
          Chicklet {
            iconText: "󰒭"
            foreground: root.foreground
            hasCursor: root.miniCursorActive && root.miniCursor === "next"
            tooltipText: "Next · Ctrl+Right"
            enabled: root.ytmusic && root.ytmusic.playbackControllable
            onClicked: if (root.ytmusic) root.ytmusic.next()
            onHovered: function(on) { if (on) root.setMiniCursor("next") }
          }
          Chicklet {
            iconText: root.ytmusic && root.ytmusic.repeatMode === "track" ? "󰑘" : "󰑖"
            foreground: root.foreground
            selected: root.ytmusic && root.ytmusic.repeatMode !== "off"
            hasCursor: root.miniCursorActive && root.miniCursor === "repeat"
            tooltipText: "Repeat: " + Api.repeatModeLabel(root.ytmusic
              ? root.ytmusic.repeatMode : "off") + " · Ctrl+R"
            enabled: root.ytmusic && root.ytmusic.playbackControllable
            onClicked: if (root.ytmusic) root.ytmusic.cycleRepeat()
            onHovered: function(on) { if (on) root.setMiniCursor("repeat") }
          }
          Chicklet {
            iconText: "󰎈"
            foreground: root.foreground
            hasCursor: root.miniCursorActive && root.miniCursor === "lyrics"
            tooltipText: "Open lyrics in Omasing · Ctrl+Shift+L"
            enabled: root.ytmusic && root.ytmusic.lyricsAvailable
            onClicked: root.openLyrics()
            onHovered: function(on) { if (on) root.setMiniCursor("lyrics") }
          }
          Chicklet {
            iconText: "󰓃"
            foreground: root.foreground
            selected: root.ytmusic && root.ytmusic.eqPreset !== "Flat"
              && root.ytmusic.eqPreset !== "Custom"
            hasCursor: root.miniCursorActive && root.miniCursor === "eq"
            tooltipText: "EQ: " + (root.ytmusic ? root.ytmusic.eqPreset : "Flat") + " · E"
            enabled: root.ytmusic && root.ytmusic.hasPlayer
            onClicked: if (root.ytmusic) root.ytmusic.cycleEqPreset()
            onHovered: function(on) { if (on) root.setMiniCursor("eq") }
          }
          }
        }

        SpectrumBar {
          width: parent.width
          visible: root.ytmusic && root.ytmusic.hasPlayer
          compact: true
          levels: root.ytmusic ? root.ytmusic.spectrumBands : []
          foreground: root.foreground
          accent: Color.accent
        }

        EqBar {
          width: parent.width
          visible: root.ytmusic && root.ytmusic.hasPlayer
          service: root.ytmusic
          foreground: root.foreground
          accent: Color.accent
          compact: true
          showPreset: false
        }

        Row {
          width: parent.width
          spacing: Style.space(8)
          visible: !root.lyricsInstallPromptVisible && root.ytmusic && root.ytmusic.hasPlayer

          Chicklet {
            iconText: root.ytmusic && root.ytmusic.volume <= 0.001 ? "󰝟" : "󰕾"
            foreground: root.foreground
            hasCursor: root.miniCursorActive && root.miniCursor === "volume"
            tooltipText: Api.controlTooltip(root.ytmusic
              && root.ytmusic.volume <= 0.001 ? "Unmute" : "Mute", "M")
            onClicked: root.toggleMute()
            onHovered: function(on) { if (on) root.setMiniCursor("volume") }
          }

          Item {
            width: Math.max(40, parent.width - Style.space(88))
            height: Style.space(28)
            anchors.verticalCenter: parent.verticalCenter
            PlaybackSlider {
              anchors.fill: parent
              anchors.leftMargin: Math.round(knobSize / 2)
              anchors.rightMargin: Math.round(knobSize / 2)
              bar: root.bar
              trackHeight: Style.space(12)
              knobSize: Style.space(18)
              minimum: 0
              maximum: 1
              step: 0.05
              sourceValue: root.ytmusic ? root.ytmusic.volume : 0.8
              contextKey: "volume"
              enabled: root.ytmusic && root.ytmusic.volumeSupported
              Accessible.role: Accessible.Slider
              Accessible.name: "Volume " + Api.volumeCaption(sourceValue)
              onCommitted: function(value) {
                if (root.ytmusic) {
                  if (Api.shouldRememberVolume(value)) root.volumeBeforeMute = value
                  root.ytmusic.setVolume(value)
                }
              }
            }
          }

          Text {
            anchors.verticalCenter: parent.verticalCenter
            text: Api.volumeCaption(root.ytmusic ? root.ytmusic.volume : 0.8)
            color: root.foreground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.caption
            font.bold: true
            width: Style.space(40)
            horizontalAlignment: Text.AlignRight
            Accessible.ignored: true
          }
        }

        PanelSeparator {
          foreground: root.foreground
          visible: !root.lyricsInstallPromptVisible
        }

        Column {
          width: parent.width
          spacing: Style.space(6)
          visible: !root.lyricsInstallPromptVisible

          RoundedField {
            id: miniSearchField
            width: parent.width
            height: Style.space(36)
            foreground: root.foreground
            areaRadius: Style.cornerRadius
            placeholderText: "Search songs to queue"
            text: root.miniSearchText
            Accessible.name: "Search songs to queue"
            Accessible.description: "Slash or Ctrl+K. Queue button adds the song, click plays it."
            onTextEdited: root.noteMiniSearch(text)
            onTextChanged: root.noteMiniSearch(text)
            onAccepted: root.runMiniSearch()
            onEditingFinished: if (String(text || "").trim() !== "") root.runMiniSearch()
            onActiveFocusChanged: if (activeFocus) root.setMiniCursor("search")
          }

          MediaCollection {
            width: parent.width
            height: root.miniSearchOpen ? Style.space(176) : 0
            visible: height > 0
            service: root.ytmusic
            sourceItems: root.miniSearchTracks
            showFilter: false
            showSort: false
            showPlaylist: false
            showSave: false
            showQueue: true
            loading: root.ytmusic && root.ytmusic.searchLoading
            emptyMessage: root.ytmusic && root.ytmusic.searchLoading
              ? "Searching…"
              : (root.ytmusic && root.ytmusic.lastError !== ""
                ? root.ytmusic.lastError
                : (String(root.miniSearchText || "").trim() === ""
                  ? "" : "No matching songs."))
            onActivated: function(item, items) {
              if (root.ytmusic) root.ytmusic.playItem(item, items)
            }
            onQueued: function(item) { root.queueMiniSearchItem(item) }
            onOpened: function(item) {
              if (root.ytmusic) root.ytmusic.playItem(item)
            }
          }
        }

        Row {
          width: parent.width
          spacing: Style.space(6)
          visible: !root.lyricsInstallPromptVisible

          Text {
            width: parent.width - openButton.width - Style.space(6)
            anchors.verticalCenter: parent.verticalCenter
            text: !root.ytmusic ? "YouTube Music is unavailable"
              : (root.ytmusic.lastError !== "" ? root.ytmusic.lastError
              : (root.ytmusic.statusMessage !== "" ? root.ytmusic.statusMessage
              : (!root.ytmusic.accountConnected
                ? "Connect YouTube Music to browse your library"
                : (root.ytmusic.playing ? "Playing on this computer"
                  : "Ready when you press play"))))
            color: Qt.darker(root.foreground, 1.35)
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.caption
            elide: Text.ElideRight
          }

          Button {
            id: openButton
            text: root.ytmusic && Api.isSignInError(root.ytmusic.lastError)
              ? "Sign in" : "Open"
            iconText: "󰏋"
            foreground: root.foreground
            hasCursor: root.miniCursorActive && root.miniCursor === "open"
            tooltipText: root.ytmusic && Api.isSignInError(root.ytmusic.lastError)
              ? "Sign in to YouTube Music" : "Open full player · O"
            onClicked: root.openSignInOrPlayer()
            onHovered: function(on) { if (on) root.setMiniCursor("open") }
          }
        }

        LyricsInstallPrompt {
          width: parent.width
          visible: root.lyricsInstallPromptVisible
          service: root.ytmusic
          foreground: root.foreground
          surfaceKey: root.lyricsRequestKey
          cancelHasCursor: root.miniCursorActive && root.miniCursor === "prompt-cancel"
          confirmHasCursor: root.miniCursorActive && root.miniCursor === "prompt-confirm"
          onCanceled: root.dismissLyricsInstallPrompt()
        }
      }

      Column {
        id: miniShortcutHelp
        anchors.fill: parent
        visible: root.miniShortcutHelpVisible
        spacing: Style.space(7)

        Row {
          width: parent.width
          spacing: Style.space(6)
          Text {
            width: parent.width - miniShortcutHelpClose.width - parent.spacing
            anchors.verticalCenter: parent.verticalCenter
            text: "Keyboard shortcuts"
            color: root.foreground
            font.family: root.bar ? root.bar.fontFamily : Style.font.family
            font.pixelSize: Style.font.subtitle
            font.bold: true
          }
          Button {
            id: miniShortcutHelpClose
            iconText: "󰅖"
            foreground: root.foreground
            focusable: true
            hasCursor: root.miniCursorActive && root.miniCursor === "help-close"
            tooltipText: "Close shortcut reference"
            Accessible.name: "Close shortcut reference"
            onClicked: root.toggleMiniShortcutHelp()
          }
        }

        PanelSeparator { foreground: root.foreground }

        Repeater {
          model: root.miniShortcutRows
          delegate: Row {
            required property var modelData
            width: miniShortcutHelp.width
            spacing: Style.space(8)
            Text {
              width: Style.space(128)
              text: modelData.keys
              color: root.foreground
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.caption
              font.bold: true
            }
            Text {
              width: parent.width - Style.space(128) - parent.spacing
              text: modelData.action
              color: Qt.darker(root.foreground, 1.35)
              font.family: root.bar ? root.bar.fontFamily : Style.font.family
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
          }
        }
      }
    }
  }

  Connections {
    target: root.ytmusic
    ignoreUnknownSignals: true
    function onLyricsPluginPromptRequested(surface, availability) {
      if (String(surface) !== root.lyricsRequestKey) return
      root.lyricsInstallPromptVisible = true
      root.popupOpen = true
    }
    function onLyricsPluginOpened(surface) {
      if (String(surface) !== root.lyricsRequestKey) return
      root.lyricsInstallPromptVisible = false
      root.close()
    }
  }
}
