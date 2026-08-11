#!/usr/bin/env node

import { spawn } from "node:child_process";

const descendant = spawn("sleep", ["60"], { stdio: "ignore" });
process.stdin.setRawMode(true);
process.stdin.resume();

function printSize() {
  process.stdout.write(`RESIZE ${process.stdout.columns}x${process.stdout.rows}\r\n`);
}

process.stdout.on("resize", printSize);
process.on("SIGWINCH", printSize);
process.stdin.on("data", (data) => {
  process.stdout.write(`INPUT_HEX ${data.toString("hex")}\r\n`);
});

process.stdout.write(
  `BOOT TERM=${process.env.TERM ?? ""} COLORTERM=${process.env.COLORTERM ?? ""} NO_COLOR=${process.env.NO_COLOR ?? ""}\r\n`,
);
process.stdout.write(`CHILD_PID=${descendant.pid}\r\n`);
process.stdout.write("\x1b[?1000h\x1b[?1002h\x1b[?1006hREADY\r\n");

