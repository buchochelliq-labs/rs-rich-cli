// The page's side of rs-rich-web: an xterm.js terminal connected to one
// session over a WebSocket, of an app or of a program on a PTY.
//
// To the server, text messages: "d" and the bytes xterm.js produced for keys,
// the mouse and pastes; "r" and "columns,rows" when the terminal's size
// changes; "c" and "number:1" (or ":0") when the clipboard took (or
// refused) an app's copy; "a" and a number of bytes once a program's output
// is drawn. From the server: what to draw, as terminal output (text for an
// app, binary for a program), and a close whose reason, if any, says why
// the session ended.
(function () {
  "use strict";

  // "app" or "program", filled in by the server.
  var mode = document.body.getAttribute("data-mode");

  var status = document.getElementById("status");
  function say(text) {
    status.textContent = text;
    status.hidden = !text;
  }

  var token = new URLSearchParams(window.location.search).get("token") || "";
  var term = new Terminal({
    cursorBlink: false,
    // An app owns the whole screen; a shell keeps what scrolled away.
    scrollback: mode === "program" ? 5000 : 0,
    macOptionIsMeta: true,
    fontFamily: 'ui-monospace, "Cascadia Mono", "DejaVu Sans Mono", Menlo, Consolas, monospace',
    fontSize: 14,
    theme: { background: "#101010" }
  });
  var fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(document.getElementById("terminal"));
  fit.fit();

  // OSC 52: text the app copied goes on the browser's clipboard. Each copy
  // is answered, by its number, with whether the clipboard took it, so the
  // app says "Copied" only when it did. Only an app's: a program's output
  // can hold anything (a file it shows, a page it fetched), and must not
  // write the clipboard behind the user's back.
  var copies = 0;
  if (mode === "app") {
    term.parser.registerOscHandler(52, function (data) {
      var number = ++copies;
      var answer = function (ok) {
        send("c" + number + ":" + (ok ? "1" : "0"));
      };
      var parts = data.split(";");
      if (parts.length < 2 || !navigator.clipboard) {
        answer(false);
        return true;
      }
      try {
        var binary = atob(parts[1]);
        var bytes = new Uint8Array(binary.length);
        for (var i = 0; i < binary.length; i++) {
          bytes[i] = binary.charCodeAt(i);
        }
        navigator.clipboard.writeText(new TextDecoder().decode(bytes)).then(
          function () { answer(true); },
          function () { answer(false); }
        );
      } catch (e) {
        // Not base64.
        answer(false);
      }
      return true;
    });
  }

  var scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  var url = scheme + "//" + window.location.host + "/ws?token=" + encodeURIComponent(token) +
    "&cols=" + term.cols + "&rows=" + term.rows + "&renderer=xterm" +
    (typeof DecompressionStream === "function" ? "&z=1" : "");
  // Compressed frames, when the server compresses (`Server::compress`):
  // each binary message starts with 0 (as it is) or 1 (deflate-raw). The
  // page asks for them only where the browser can undo them. Messages are
  // handled one after another, in order, as decompressing is async.
  var compressed = typeof DecompressionStream === "function";
  var queue = Promise.resolve();
  function unpack(data) {
    if (typeof data === "string" || !compressed) {
      return Promise.resolve(typeof data === "string" ? data : new Uint8Array(data));
    }
    var bytes = new Uint8Array(data);
    if (bytes[0] !== 1) {
      return Promise.resolve(bytes.subarray(1));
    }
    var stream = new Blob([bytes.subarray(1)]).stream()
      .pipeThrough(new DecompressionStream("deflate-raw"));
    return new Response(stream).arrayBuffer().then(function (out) {
      return new Uint8Array(out);
    });
  }

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
    queue = queue.then(function () {
      return unpack(event.data);
    }).then(function (data) {
      if (typeof data === "string") {
        term.write(data);
      } else {
        // A program's output: say once it is drawn, so the server sends
        // more only as fast as the page keeps up.
        term.write(data, function () {
          send("a" + data.length);
        });
      }
    }, function () {
      // A frame that does not decompress is dropped.
    });
  };
  socket.onclose = function (event) {
    var was = open;
    open = false;
    term.options.disableStdin = true;
    if (was && event.reason) {
      say(event.reason + " Reload the page to start a new session.");
    } else if (was) {
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
