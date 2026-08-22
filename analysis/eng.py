#!/usr/bin/env python3
"""Persistent-process UCI driver. Holds stdin open, waits for bestmove."""
import subprocess, threading, queue, time
from pathlib import Path

class UciEngine:
    def __init__(self, exe, options=None):
        self.p = subprocess.Popen([str(exe)], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                  text=True, bufsize=1)
        self.q = queue.Queue()
        threading.Thread(target=self._reader, daemon=True).start()
        self.send("uci"); self.wait_for("uciok")
        for name, value in (options or {}).items():
            self.setoption(name, value)
        self.send("isready"); self.wait_for("readyok")

    def _reader(self):
        for line in self.p.stdout:
            self.q.put(line.rstrip("\n"))
        self.q.put(None)  # EOF marker

    def send(self, cmd):
        self.p.stdin.write(cmd + "\n")
        self.p.stdin.flush()

    def setoption(self, name, value):
        self.send("setoption name %s value %s" % (name, value))

    def wait_for(self, token, timeout=1800):
        """Read lines until one starts with token. Returns collected lines."""
        end = time.time() + timeout
        lines = []
        while True:
            remaining = end - time.time()
            if remaining <= 0:
                raise TimeoutError("waiting for %r, got: %r" % (token, lines[-5:]))
            try:
                line = self.q.get(timeout=remaining)
            except queue.Empty:
                continue
            if line is None:
                raise EOFError("engine died; last lines: %r" % lines[-5:])
            lines.append(line)
            if line.startswith(token):
                return lines

    def newgame(self):
        self.send("ucinewgame")
        self.send("isready"); self.wait_for("readyok")

    def position(self, fen=None, moves=None, startpos=False):
        if startpos:
            pos = "position startpos"
        else:
            pos = "position fen " + fen
        if moves:
            pos += " moves " + " ".join(moves)
        self.send(pos)

    def go(self, **kw):
        """go(depth=N | movetime=ms | wtime/btime/pairs). Waits for bestmove.
        Returns (info_lines, bestmove)."""
        parts = ["go"]
        for k, v in kw.items():
            parts += [k, str(v)]
        self.send(" ".join(parts))
        lines = self.wait_for("bestmove")
        best = lines[-1].split()[1] if len(lines[-1].split()) > 1 else None
        return lines[:-1], best

    def display(self):
        """Send 'd' and capture through 'Checkers:' (last line of d output)."""
        self.send("d")
        lines = self.wait_for("Checkers:")
        fen = None
        for l in lines:
            if l.startswith("Fen:"):
                fen = l[5:].strip()
        return fen, lines

    def moves(self, fen):
        """Legal moves at a position via 'go perft 1' divide listing."""
        self.position(fen=fen)
        self.send("go perft 1")
        lines = self.wait_for("Nodes searched")
        return [l.split(":")[0] for l in lines if ": " in l and not l.startswith("Nodes")]

    def perft(self, fen, depth):
        self.position(fen=fen)
        self.send("go perft %d" % depth)
        # perft output ends with 'Nodes searched: N' (no bestmove follows in FSF)
        lines = self.wait_for("Nodes searched")
        return int(lines[-1].split()[-1])

    def quit(self):
        try:
            self.send("quit")
            self.p.wait(timeout=5)
        except Exception:
            self.p.kill()

if __name__ == "__main__":
    # Single-variable isolation of the quit-after-go artifact:
    exe = Path(__file__).parent.parent / "engine" / "fairy-stockfish.exe"

    # A) broken pattern: all input piped at once (simulated by immediate quit)
    t0 = time.time()
    p = subprocess.run([str(exe)], input="uci\nposition startpos\ngo movetime 3000\nquit\n",
                       capture_output=True, text=True)
    tA = time.time() - t0
    infoA = sum(1 for l in p.stdout.splitlines() if l.startswith("info depth"))

    # B) held-open pattern via driver
    eng = UciEngine(exe)
    t0 = time.time()
    eng.position(startpos=True)
    infoB, bestB = eng.go(movetime=3000)
    tB = time.time() - t0
    eng.quit()

    print("A) pipe+immediate-quit : %.2fs, info-depth lines=%d, %s" %
          (tA, infoA, [l for l in p.stdout.splitlines() if l.startswith('bestmove')]))
    print("B) held-open driver    : %.2fs, info-depth lines=%d, %s" %
          (tB, len(infoB), "bestmove " + str(bestB)))
