import QtQuick
import Quickshell
import "__PLUGIN_URL__" as Music

Scope {
  id: test
  property bool failed: false
  property int step: 0

  function check(condition, message) {
    if (!condition) {
      failed = true
      console.error("DETAIL_LOADING_FAIL", message)
    }
  }

  Music.Service {
    id: service
    property var testReply: null
    function command(name, fields, message, done) { testReply = done }
  }

  Timer {
    interval: 20
    running: true
    onTriggered: {
      service.openDetail({ type: "playlist", id: "PLtest", kind: "context" })
      test.check(service.detailLoading, "Playlist must start loading")
      reply.start()
    }
  }

  Timer {
    id: reply
    interval: 20
    onTriggered: {
      try {
        if (test.step === 0) {
          service.testReply(true, { type: "playlist", tracks: [{ id: "song" }] })
          test.check(!service.detailLoading, "Playlist response must clear loading")
          test.check(service.detailTracks.length === 1, "Playlist tracks must display")
          service.openDetail({ type: "album", id: "MPREb_test", kind: "context" })
          test.check(service.detailLoading, "Album must start loading")
        } else if (test.step === 1) {
          service.testReply(true, { type: "album", tracks: [{ id: "album-song" }] })
          test.check(!service.detailLoading, "Album response must clear loading")
          test.check(service.detailTracks[0].id === "album-song", "Album tracks must display")
          service.openDetail({ type: "playlist", id: "PLfailed", kind: "context" })
        } else if (test.step === 2) {
          service.testReply(false, null)
          test.check(!service.detailLoading, "Failed response must clear loading")
          service.openDetail({ type: "playlist", id: "PLold", kind: "context" })
          var staleReply = service.testReply
          service.openDetail({ type: "album", id: "MPREb_new", kind: "context" })
          staleReply(true, { type: "playlist", tracks: [{ id: "stale" }] })
          test.check(service.detailLoading, "Stale response must not stop a newer request")
          test.check(service.detailTracks.length === 0, "Stale tracks must not display")
        } else {
          service.testReply(true, { type: "album", tracks: [{ id: "new" }] })
          test.check(!service.detailLoading, "Latest response must clear loading")
          test.check(service.detailTracks[0].id === "new", "Latest tracks must display")
          if (!test.failed) console.log("DETAIL_LOADING_PASS")
          Qt.quit()
          return
        }
        test.step++
        reply.restart()
      } catch (error) {
        console.error("DETAIL_LOADING_FAIL", String(error))
        Qt.quit()
      }
    }
  }
}
