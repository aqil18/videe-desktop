/*
 * Videe <-> Adobe Premiere Pro bridge: the ExtendScript side, called by main.js.
 * Mirrors create_timeline in integrations/resolve/Videe.lua.
 *
 * ExtendScript is ES3 (no JSON, no Array indexOf, no String trim), and Premiere
 * runs every panel's scripts in one shared engine, so everything lives on a
 * single VideeBridge object to stay out of other extensions' way.
 *
 * Results go back as plain strings, "ok" or "error:<message>", because there's
 * no JSON.stringify here to build anything richer.
 */
var VideeBridge = (function () {
    // Premiere's marker colour indexes: 0 green, 1 red, 2 purple, 3 orange,
    // 4 yellow, 5 white, 6 blue, 7 cyan. Blue matches the Resolve bridge.
    var MARKER_BLUE = 6;
    var BIN_NAME = "Videe";

    function splitLines(str) {
        var out = [];
        var parts = String(str || "").split("\n");
        for (var i = 0; i < parts.length; i++) {
            if (parts[i] !== "") out.push(parts[i]);
        }
        return out;
    }

    function basename(path) {
        var m = String(path).match(/([^\/\\]+)$/);
        return m ? m[1] : String(path);
    }

    // Plain objects as sets keyed by path, prefixed so a path can't collide
    // with a built-in property name.
    function key(path) {
        return "p:" + path;
    }

    function mediaPath(item) {
        try {
            return item.getMediaPath();
        } catch (e) {
            return "";
        }
    }

    // Markers arrive as one string, a line per marker: "<clip>\t<offsetSeconds>\t<name>",
    // where <clip> is the 1-based position in the clip list and the offset is
    // seconds from that clip's start. See bridge.rs::encode_markers.
    function parseMarkers(str) {
        var markers = [];
        var lines = splitLines(str);
        for (var i = 0; i < lines.length; i++) {
            var m = lines[i].match(/^(\d+)\t([^\t]+)\t([\s\S]*)$/);
            if (m && !isNaN(parseFloat(m[2]))) {
                markers.push({ clip: parseInt(m[1], 10), offset: parseFloat(m[2]), name: m[3] });
            }
        }
        return markers;
    }

    // Fills found[key(path)] for every project clip whose file is one of
    // `wanted`, searching every bin, so sending the same clips twice reuses
    // them instead of importing duplicates.
    function findClipsByPath(bin, wanted, found) {
        for (var i = 0; i < bin.children.numItems; i++) {
            var item = bin.children[i];
            if (!item) continue;
            if (item.type === ProjectItemType.BIN) {
                findClipsByPath(item, wanted, found);
            } else if (item.type === ProjectItemType.CLIP) {
                var k = key(mediaPath(item));
                if (wanted.hasOwnProperty(k) && !found.hasOwnProperty(k)) found[k] = item;
            }
        }
    }

    function findOrCreateBin(name) {
        var root = app.project.rootItem;
        for (var i = 0; i < root.children.numItems; i++) {
            var item = root.children[i];
            if (item && item.type === ProjectItemType.BIN && item.name === name) return item;
        }
        return root.createBin(name);
    }

    // Returns { items, missing } for `paths`, in order, importing any that
    // aren't in the project yet. Importing by absolute path links the exact
    // file, so there's no conform step that could leave a clip offline.
    function projectItems(paths) {
        var wanted = {};
        var found = {};
        var i;
        for (i = 0; i < paths.length; i++) wanted[key(paths[i])] = true;
        findClipsByPath(app.project.rootItem, wanted, found);

        var toImport = [];
        for (i = 0; i < paths.length; i++) {
            if (!found.hasOwnProperty(key(paths[i]))) toImport.push(paths[i]);
        }

        if (toImport.length > 0) {
            var bin = findOrCreateBin(BIN_NAME);
            var before = {};
            for (i = 0; i < bin.children.numItems; i++) before[bin.children[i].nodeId] = true;

            app.project.importFiles(toImport, true, bin, false);

            // Match what was just imported by path, falling back to file name in
            // case Premiere reports a normalised path (e.g. a resolved symlink).
            var byName = {};
            for (i = 0; i < bin.children.numItems; i++) {
                var item = bin.children[i];
                if (!item || before[item.nodeId] || item.type !== ProjectItemType.CLIP) continue;
                var k = key(mediaPath(item));
                if (wanted.hasOwnProperty(k) && !found.hasOwnProperty(k)) found[k] = item;
                byName[key(item.name)] = item;
            }
            for (i = 0; i < toImport.length; i++) {
                var pk = key(toImport[i]);
                var nk = key(basename(toImport[i]));
                if (!found.hasOwnProperty(pk) && byName.hasOwnProperty(nk)) found[pk] = byName[nk];
            }
        }

        var items = [];
        var missing = [];
        for (i = 0; i < paths.length; i++) {
            var fk = key(paths[i]);
            if (found.hasOwnProperty(fk)) items.push(found[fk]);
            else missing.push(basename(paths[i]));
        }
        return { items: items, missing: missing };
    }

    // Adds sequence markers, each placed relative to where Premiere actually put
    // its clip on V1, so this doesn't depend on the sequence's start timecode or
    // frame rate. Accounts for a source in-point in case a reused project item
    // already had in/out marks. Returns how many couldn't be added.
    function addMarkers(sequence, markers) {
        if (markers.length === 0) return 0;

        var track = sequence.videoTracks[0];
        var trackItems = [];
        for (var i = 0; i < track.clips.numItems; i++) trackItems.push(track.clips[i]);
        trackItems.sort(function (a, b) {
            return a.start.seconds - b.start.seconds;
        });

        var failed = 0;
        for (var j = 0; j < markers.length; j++) {
            var marker = markers[j];
            var item = trackItems[marker.clip - 1];
            var added = false;
            if (item) {
                var inPoint = item.inPoint.seconds;
                var outPoint = item.outPoint.seconds;
                if (marker.offset >= inPoint && marker.offset <= outPoint) {
                    var created = sequence.markers.createMarker(item.start.seconds + marker.offset - inPoint);
                    if (created) {
                        created.name = marker.name;
                        try {
                            created.setColorByIndex(MARKER_BLUE);
                        } catch (e) {
                            // Older Premiere builds lack marker colours; the marker still counts.
                        }
                        added = true;
                    }
                }
            }
            if (!added) failed++;
        }
        return failed;
    }

    function createSequenceImpl(req) {
        if (!app.project || !app.project.rootItem) {
            return "error:No project is open in Premiere Pro";
        }

        var paths = splitLines(req.clips);
        if (paths.length === 0) return "error:No clips were sent";

        var resolved = projectItems(paths);
        if (resolved.missing.length > 0) {
            return (
                "error:Premiere couldn't import " +
                resolved.missing.join(", ") +
                " (is the file still there, and a format Premiere supports?)"
            );
        }

        var bin = findOrCreateBin(BIN_NAME);
        var sequence = app.project.createNewSequenceFromClips(req.timelineName, resolved.items, bin);
        if (!sequence) return "error:Premiere could not create the sequence";

        try {
            app.project.openSequence(sequence.sequenceID);
        } catch (e) {
            // Creating it usually opens it anyway; not worth failing over.
        }

        var wanted = parseMarkers(req.markers);
        var failed;
        try {
            failed = addMarkers(sequence, wanted);
        } catch (e) {
            return "error:Sequence created, but adding markers failed: " + e.toString();
        }
        if (failed > 0) {
            return "error:Sequence created, but " + failed + " of " + wanted.length + " timestamps couldn't be added as markers";
        }
        return "ok";
    }

    return {
        createSequence: function (req) {
            try {
                return createSequenceImpl(req);
            } catch (e) {
                return "error:Premiere error: " + e.toString() + (e.line ? " (line " + e.line + ")" : "");
            }
        },
    };
})();
