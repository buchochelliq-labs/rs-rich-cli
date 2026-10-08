// The page's side of rs-rich-web: an xterm.js terminal connected to one app
// session over a WebSocket.
//
// To the server, text messages: "d" and the bytes xterm.js produced for keys,
// the mouse and pastes; "r" and "columns,rows" when the terminal's size
// changes. From the server: what to draw, as terminal output.
(function () {
  "use strict";

  var status = document.getElementById("status");
  function say(text) {
    status.textContent = text;
    status.hidden = !text;
  }

  var token = new URLSearchParams(window.location.search).get("token") || "";
  var term = new Terminal({
    cursorBlink: false,
    scrollback: 0,
    macOptionIsMeta: true,
    fontFamily: 'ui-monospace, "Cascadia Mono", "DejaVu Sans Mono", Menlo, Consolas, monospace',
    fontSize: 14,
    theme: { background: "#101010" }
  });
  var fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(document.getElementById("terminal"));
  fit.fit();

  // OSC 52: text the app copied goes on the browser's clipboard.
  term.parser.registerOscHandler(52, function (data) {
    var parts = data.split(";");
    if (parts.length < 2 || !navigator.clipboard) {
      return true;
    }
    try {
      var binary = atob(parts[1]);
      var bytes = new Uint8Array(binary.length);
      for (var i = 0; i < binary.length; i++) {
        bytes[i] = binary.charCodeAt(i);
      }
      navigator.clipboard.writeText(new TextDecoder().decode(bytes)).catch(function () {});
    } catch (e) {
      // Not base64: ignore it.
    }
    return true;
  });

  var scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  var url = scheme + "//" + window.location.host + "/ws?token=" + encodeURIComponent(token) +
    "&cols=" + term.cols + "&rows=" + term.rows;
  var socket = new WebSocket(url);
  socket.binaryType = "arraybuffer";
  var open = false;

  function send(message) {
    if (open && socket.readyState === WebSocket.OPEN) {
      socket.send(message);
    }
  }

  socket.onopen = function () {
    open = true;
    say("");
    // The size may have changed while connecting.
    send("r" + term.cols + "," + term.rows);
    term.focus();
  };
  socket.onmessage = function (event) {
    if (typeof event.data === "string") {
      term.write(event.data);
    } else {
      term.write(new Uint8Array(event.data));
    }
  };
  socket.onclose = function (event) {
    var was = open;
    open = false;
    term.options.disableStdin = true;
    if (was) {
      say("The session ended. Reload the page to start a new one.");
    } else {
      say("Could not connect" + (event.reason ? ": " + event.reason : "") +
        ". Open the address the server printed, with its token.");
    }
  };

  term.onData(function (data) {
    send("d" + data);
  });
  term.onResize(function (size) {
    send("r" + size.cols + "," + size.rows);
  });

  var pending = false;
  window.addEventListener("resize", function () {
    if (!pending) {
      pending = true;
      window.requestAnimationFrame(function () {
        pending = false;
        fit.fit();
      });
    }
  });

  say("Connecting…");
})();
