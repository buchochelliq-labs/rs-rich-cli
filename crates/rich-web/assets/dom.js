// The page's side of rs-rich-web's DOM renderer: an app drawn as a grid of
// styled spans, with its accessibility tree laid over the grid as ARIA, and
// its announcements in live regions. The protocol (version 1) is described
// in the crate's `dom` module.
//
// To the server, text messages, as the xterm.js page sends them: "d" and the
// bytes an xterm sends for a key, a mouse report or a paste; "r" and
// "columns,rows" when the page's size changes; "c" and "number:1" (or ":0")
// when the clipboard took (or refused) a copy. From the server: JSON
// messages (hello, frame, tree, say, copy), then a close.
(function () {
  "use strict";

  var PROTOCOL = 1;
  var CSI = "\x1b[";

  var screen = document.getElementById("screen");
  var grid = document.getElementById("grid");
  var caret = document.getElementById("caret");
  var probe = document.getElementById("probe");
  var root = document.getElementById("tree");
  var polite = document.getElementById("polite");
  var assertive = document.getElementById("assertive");
  var status = document.getElementById("status");
  var sheet = document.getElementById("styles").sheet;

  function say(text) {
    status.textContent = text;
    status.hidden = !text;
  }

  // The size of a cell, in pixels, from ten characters of the grid's font.
  var cw = 8, rh = 17;
  function measure() {
    var box = probe.getBoundingClientRect();
    if (box.width > 0 && box.height > 0) {
      cw = box.width / 10;
      rh = box.height;
    }
    document.documentElement.style.setProperty("--cw", cw + "px");
    document.documentElement.style.setProperty("--rh", rh + "px");
  }
  function fit() {
    var box = screen.getBoundingClientRect();
    return [
      Math.max(1, Math.min(1000, Math.floor(box.width / cw))),
      Math.max(1, Math.min(1000, Math.floor(box.height / rh)))
    ];
  }
  measure();
  var size = fit();
  var cols = 0, rows = 0;

  var token = new URLSearchParams(window.location.search).get("token") || "";
  var scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
  var socket = new WebSocket(scheme + "//" + window.location.host + "/ws?token=" +
    encodeURIComponent(token) + "&cols=" + size[0] + "&rows=" + size[1] + "&renderer=dom");
  var open = false;

  function send(message) {
    if (open && socket.readyState === WebSocket.OPEN) {
      socket.send(message);
    }
  }

  // The grid: one element per line, one span per run of a style.
  var lines = [];
  function resize(c, r) {
    cols = c;
    rows = r;
    while (lines.length < r) {
      var line = document.createElement("div");
      line.className = "line";
      grid.appendChild(line);
      lines.push(line);
    }
    while (lines.length > r) {
      grid.removeChild(lines.pop());
    }
  }
  function drawLine(y, runs) {
    var line = lines[y];
    if (!line) {
      return;
    }
    var spans = document.createDocumentFragment();
    runs.forEach(function (run) {
      var span = document.createElement("span");
      span.textContent = run[0];
      var names = [];
      if (run[1]) {
        names.push("s" + run[1]);
      }
      if (run[2] === 2) {
        names.push("w");
      }
      span.className = names.join(" ");
      spans.appendChild(span);
    });
    line.replaceChildren(spans);
  }
  function frame(message) {
    if (message.cols !== cols || message.rows !== rows) {
      resize(message.cols, message.rows);
    }
    message.styles.forEach(function (style) {
      try {
        sheet.insertRule(".s" + style[0] + " { " + style[1] + " }", sheet.cssRules.length);
      } catch (e) {
        // A declaration this browser does not take: the run stays plain.
      }
    });
    message.lines.forEach(function (line) {
      drawLine(line[0], line[1]);
    });
    if (message.cursor) {
      caret.style.left = message.cursor[0] * cw + "px";
      caret.style.top = message.cursor[1] * rh + "px";
      caret.hidden = false;
    } else {
      caret.hidden = true;
    }
  }

  // The accessibility tree: an element per node, kept between messages (so
  // focus stays where it is), nested by depth, placed over its cells.
  var elements = {};
  function element(key, seen) {
    seen[key] = true;
    var el = elements[key];
    if (!el) {
      el = document.createElement("div");
      el.textNode = document.createTextNode("");
      el.appendChild(el.textNode);
      el.attrNames = [];
      elements[key] = el;
    }
    return el;
  }
  function apply(el, attrs, text) {
    var names = attrs.map(function (pair) { return pair[0]; });
    el.attrNames.forEach(function (name) {
      if (names.indexOf(name) < 0) {
        el.removeAttribute(name);
      }
    });
    attrs.forEach(function (pair) {
      if (el.getAttribute(pair[0]) !== pair[1]) {
        el.setAttribute(pair[0], pair[1]);
      }
      // A status or a log would announce its changes itself: the server's
      // announcements already say them, once.
      if (pair[0] === "role" && (pair[1] === "status" || pair[1] === "log")) {
        el.setAttribute("aria-live", "off");
        names.push("aria-live");
      }
    });
    el.attrNames = names;
    if (el.textNode.data !== text) {
      el.textNode.data = text;
    }
  }
  function place(parent, el) {
    var at = parent.el.children[parent.index];
    if (at !== el) {
      parent.el.insertBefore(el, at || null);
    }
    parent.index++;
  }
  function tree(message) {
    var seen = {};
    var stack = [{ el: root, depth: -1, rect: [0, 0, 0, 0], index: 0 }];
    message.nodes.forEach(function (node) {
      while (stack.length > 1 && stack[stack.length - 1].depth >= node.depth) {
        stack.pop();
      }
      var parent = stack[stack.length - 1];
      var el = element("n" + node.id, seen);
      apply(el, node.attrs, node.text);
      var r = node.rect;
      el.style.left = (r[0] - parent.rect[0]) * cw + "px";
      el.style.top = (r[1] - parent.rect[1]) * rh + "px";
      el.style.width = r[2] * cw + "px";
      el.style.height = r[3] * rh + "px";
      place(parent, el);
      var entry = { el: el, depth: node.depth, rect: r, index: 0 };
      if (node.item) {
        var item = element("i" + node.id, seen);
        item.className = "item";
        apply(item, [["role", node.item.role]].concat(node.item.attrs), node.item.text);
        place(entry, item);
      }
      stack.push(entry);
    });
    Object.keys(elements).forEach(function (key) {
      if (!seen[key]) {
        var el = elements[key];
        if (el.parentNode) {
          el.parentNode.removeChild(el);
        }
        delete elements[key];
      }
    });
    focus(message.focus);
  }
  var focused = null;
  function focus(id) {
    var el = (id !== null && (elements["i" + id] || elements["n" + id])) || root;
    if (focused && focused !== el && focused !== root) {
      focused.removeAttribute("tabindex");
    }
    if (el !== root) {
      el.setAttribute("tabindex", "0");
    }
    focused = el;
    if (document.activeElement !== el) {
      el.focus({ preventScroll: true });
    }
  }

  // Announcements: a new line in a live region, which a screen reader
  // reads as it is added.
  function announce(message) {
    var region = message.urgent ? assertive : polite;
    var line = document.createElement("div");
    line.textContent = message.text;
    region.appendChild(line);
    while (region.children.length > 3) {
      region.removeChild(region.firstChild);
    }
  }

  // A copy: onto the clipboard, then the answer, so the app says "Copied"
  // only when it worked.
  function copy(message) {
    var answer = function (ok) {
      send("c" + message.n + ":" + (ok ? "1" : "0"));
    };
    if (!navigator.clipboard) {
      answer(false);
      return;
    }
    navigator.clipboard.writeText(message.text).then(
      function () { answer(true); },
      function () { answer(false); }
    );
  }

  socket.onopen = function () {
    open = true;
    say("");
    root.focus({ preventScroll: true });
  };
  socket.onmessage = function (event) {
    var message;
    try {
      message = JSON.parse(event.data);
    } catch (e) {
      return;
    }
    switch (message.t) {
      case "hello":
        if (message.protocol !== PROTOCOL) {
          say("This page and the server speak different versions. Reload the page.");
          socket.close();
        }
        break;
      case "frame": frame(message); break;
      case "tree": tree(message); break;
      case "say": announce(message); break;
      case "copy": copy(message); break;
    }
  };
  socket.onclose = function (event) {
    var was = open;
    open = false;
    if (was) {
      say("The session ended. Reload the page to start a new one.");
    } else {
      say("Could not connect" + (event.reason ? ": " + event.reason : "") +
        ". Open the address the server printed, with its token.");
    }
  };

  // Keys, as the bytes an xterm sends for them, so the server reads them as
  // it reads the xterm.js page's.
  var ARROWS = { ArrowUp: "A", ArrowDown: "B", ArrowRight: "C", ArrowLeft: "D", Home: "H", End: "F" };
  var SS3 = { F1: "P", F2: "Q", F3: "R", F4: "S" };
  var TILDE = {
    Insert: 2, Delete: 3, PageUp: 5, PageDown: 6, F5: 15, F6: 17, F7: 18, F8: 19,
    F9: 20, F10: 21, F11: 23, F12: 24
  };
  var CONTROL = { " ": "\x00", "@": "\x00", "2": "\x00", "[": "\x1b", "\\": "\x1c",
    "]": "\x1d", "^": "\x1e", "6": "\x1e", "_": "\x1f", "/": "\x1f" };
  function keyBytes(e) {
    var key = e.key;
    if (e.metaKey || e.isComposing || key === "Dead" || key === "Unidentified") {
      return null;
    }
    var m = 1 + (e.shiftKey ? 1 : 0) + (e.altKey ? 2 : 0) + (e.ctrlKey ? 4 : 0);
    var alt = e.altKey ? "\x1b" : "";
    if (ARROWS[key]) {
      return m > 1 ? CSI + "1;" + m + ARROWS[key] : CSI + ARROWS[key];
    }
    if (SS3[key]) {
      return m > 1 ? CSI + "1;" + m + SS3[key] : "\x1bO" + SS3[key];
    }
    if (TILDE[key]) {
      return CSI + TILDE[key] + (m > 1 ? ";" + m : "") + "~";
    }
    switch (key) {
      case "Enter": return alt + "\r";
      case "Tab": return e.shiftKey ? CSI + "Z" : "\t";
      case "Backspace": return alt + (e.ctrlKey ? "\x08" : "\x7f");
      case "Escape": return "\x1b";
    }
    if (Array.from(key).length !== 1) {
      return null;
    }
    // AltGr: the character it makes.
    if (e.getModifierState && e.getModifierState("AltGraph")) {
      return key;
    }
    if (e.ctrlKey) {
      var lower = key.toLowerCase();
      // Ctrl+Shift+C and Ctrl+Shift+V stay the browser's: copy and paste.
      if (e.shiftKey && lower >= "a" && lower <= "z") {
        return null;
      }
      if (lower.length === 1 && lower >= "a" && lower <= "z") {
        return alt + String.fromCharCode(lower.charCodeAt(0) - 96);
      }
      return CONTROL[key] !== undefined ? alt + CONTROL[key] : null;
    }
    return alt + key;
  }
  document.addEventListener("keydown", function (e) {
    var bytes = keyBytes(e);
    if (bytes !== null && open) {
      e.preventDefault();
      send("d" + bytes);
    }
  });
  document.addEventListener("paste", function (e) {
    var text = e.clipboardData ? e.clipboardData.getData("text/plain") : "";
    if (text && open) {
      e.preventDefault();
      send("d" + CSI + "200~" + text + CSI + "201~");
    }
  });

  // The mouse, as SGR reports on the cell under the pointer.
  function cell(e) {
    var box = grid.getBoundingClientRect();
    var x = Math.floor((e.clientX - box.left) / cw);
    var y = Math.floor((e.clientY - box.top) / rh);
    return [Math.max(0, Math.min(cols - 1, x)) + 1, Math.max(0, Math.min(rows - 1, y)) + 1];
  }
  function report(code, e, at, press) {
    code += (e.shiftKey ? 4 : 0) + (e.altKey ? 8 : 0) + (e.ctrlKey ? 16 : 0);
    send("d" + CSI + "<" + code + ";" + at[0] + ";" + at[1] + (press ? "M" : "m"));
  }
  var held = -1, last = null, wheel = 0;
  screen.addEventListener("mousedown", function (e) {
    if (e.button > 2) {
      return;
    }
    e.preventDefault();
    held = e.button;
    last = cell(e);
    report(held, e, last, true);
    if (focused) {
      focused.focus({ preventScroll: true });
    }
  });
  document.addEventListener("mouseup", function (e) {
    if (held < 0) {
      return;
    }
    report(held, e, cell(e), false);
    held = -1;
  });
  screen.addEventListener("mousemove", function (e) {
    var at = cell(e);
    if (last && at[0] === last[0] && at[1] === last[1]) {
      return;
    }
    last = at;
    report(held >= 0 ? 32 + held : 35, e, at, true);
  });
  screen.addEventListener("wheel", function (e) {
    e.preventDefault();
    wheel += e.deltaMode === 1 ? e.deltaY * rh : e.deltaY;
    while (Math.abs(wheel) >= rh) {
      report(wheel < 0 ? 64 : 65, e, cell(e), true);
      wheel -= wheel < 0 ? -rh : rh;
    }
  }, { passive: false });
  screen.addEventListener("contextmenu", function (e) {
    e.preventDefault();
  });

  // The window's size, as cells.
  var pending = false;
  window.addEventListener("resize", function () {
    if (pending) {
      return;
    }
    pending = true;
    window.requestAnimationFrame(function () {
      pending = false;
      measure();
      var next = fit();
      if (next[0] !== size[0] || next[1] !== size[1]) {
        size = next;
        send("r" + size[0] + "," + size[1]);
      }
    });
  });

  say("Connecting…");
})();
