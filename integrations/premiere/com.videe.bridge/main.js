/*
 * Videe <-> Adobe Premiere Pro bridge: the panel side.
 *
 * Same shape as the DaVinci Resolve bridge (integrations/resolve/Videe.lua):
 * this panel is the server, Videe is the client, and it only does one thing.
 * When Videe POSTs a CreateTimeline request, it imports those files into the
 * open project, builds a sequence from them, and marks their timestamps. The
 * Premiere work happens in host.jsx (ExtendScript). This file only runs the
 * HTTP server (CEP's Node.js integration) and hands requests to ExtendScript.
 *
 * The wire format matches the Resolve bridge exactly (see src-tauri/src/bridge.rs),
 * so Videe sends the same request to either NLE.
 *
 * The server lives as long as the panel does. Premiere reopens docked panels
 * with the workspace, so once it's docked it starts with Premiere.
 */
(function () {
  "use strict";

  var PORT = 51736;
  var MAX_BODY_BYTES = 8 * 1024 * 1024;

  var http = require("http");
  var statusEl = document.getElementById("status");
  var lastEl = document.getElementById("last");

  function setStatus(text, kind) {
    statusEl.textContent = text;
    statusEl.className = kind || "";
  }

  function setLast(text) {
    lastEl.textContent = text;
  }

  // ExtendScript is ES3: no JSON.parse. JSON is valid JavaScript literal
  // syntax, except that ES3 treats U+2028/U+2029 inside strings as line breaks,
  // so escape those and the request can be passed as a literal argument.
  function toExtendScriptLiteral(value) {
    return JSON.stringify(value).replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029");
  }

  function evalHost(script) {
    return new Promise(function (resolve) {
      window.__adobe_cep__.evalScript(script, resolve);
    });
  }

  // host.jsx returns "ok", or "error:<message>". Anything else means
  // ExtendScript itself failed (CEP answers "EvalScript error.").
  function parseHostResult(result) {
    if (result === "ok") return { ok: true };
    if (typeof result === "string" && result.indexOf("error:") === 0) {
      return { ok: false, error: result.slice("error:".length) };
    }
    return { ok: false, error: "Premiere's scripting engine failed: " + String(result) };
  }

  function createTimeline(req) {
    var args = {
      clips: String(req.clips || ""),
      timelineName: String(req.timelineName || "Videe Export"),
      markers: String(req.markers || ""),
    };
    return evalHost("VideeBridge.createSequence(" + toExtendScriptLiteral(args) + ")").then(parseHostResult);
  }

  function handleRequest(req) {
    if (!req || typeof req.func !== "string") {
      return Promise.resolve({ ok: false, error: "malformed request" });
    }
    switch (req.func) {
      case "Ping":
        return Promise.resolve({ ok: true });
      case "CreateTimeline":
        return createTimeline(req);
      default:
        return Promise.resolve({ ok: false, error: "unknown func: " + req.func });
    }
  }

  function send(res, status, body) {
    var json = JSON.stringify(body);
    res.writeHead(status, { "Content-Type": "application/json", "Content-Length": Buffer.byteLength(json) });
    res.end(json);
  }

  // Only Videe should be able to drive this. Browsers can't send a cross-origin
  // POST with Content-Type: application/json without a CORS preflight (which
  // this never approves), always attach Origin to one, and a DNS-rebinding page
  // would carry its own Host. Videe's HTTP client does none of that.
  function isFromVidee(req) {
    var host = req.headers.host;
    var type = req.headers["content-type"] || "";
    return (
      req.method === "POST" &&
      !req.headers.origin &&
      (host === "127.0.0.1:" + PORT || host === "localhost:" + PORT) &&
      type.split(";")[0].trim() === "application/json"
    );
  }

  var server = http.createServer(function (req, res) {
    if (!isFromVidee(req)) {
      send(res, 403, { ok: false, error: "forbidden" });
      req.resume();
      return;
    }

    var chunks = [];
    var size = 0;
    var aborted = false;
    req.on("data", function (chunk) {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        aborted = true;
        send(res, 413, { ok: false, error: "request too large" });
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on("end", function () {
      if (aborted) return;
      var parsed;
      try {
        parsed = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      } catch (e) {
        send(res, 200, { ok: false, error: "malformed request" });
        return;
      }
      handleRequest(parsed).then(
        function (result) {
          if (parsed.func === "CreateTimeline") {
            setLast(
              (result.ok ? "Created " : "Failed: ") +
                (result.ok ? parsed.timelineName : result.error) +
                " (" + new Date().toLocaleTimeString() + ")"
            );
          }
          send(res, 200, result);
        },
        function (err) {
          send(res, 200, { ok: false, error: "Videe panel error: " + String(err) });
        }
      );
    });
  });

  server.on("error", function (err) {
    if (err.code === "EADDRINUSE") {
      setStatus(
        "Port " + PORT + " is in use. Close any other Videe panel (another Premiere window?), then close and reopen this one.",
        "error"
      );
    } else {
      setStatus("Couldn't start: " + err.message, "error");
    }
  });

  server.listen(PORT, "127.0.0.1", function () {
    setStatus("Ready for Videe on 127.0.0.1:" + PORT, "ok");
  });

  // Release the port when the panel closes so reopening it (e.g. after Videe
  // installs an updated copy) can bind again straight away.
  window.addEventListener("beforeunload", function () {
    server.close();
  });
})();
