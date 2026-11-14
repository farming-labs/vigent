#!/usr/bin/env node
/**
 * Fail when a linux-gnu vigent needs a newer glibc than it's meant to run on.
 * It reads the glibc symbol versions the binary asks for (`objdump -T`) and its
 * version references (`objdump -p`, which also lists the weak ones a loader
 * still refuses: "weak version 'GLIBC_2.39' not found").
 *
 *   node scripts/check-glibc.mjs <binary> <glibc>     e.g. dist-bin/vigent-linux-x64/vigent 2.17
 *
 * Needs binutils' objdump (any architecture's binary: it only reads the ELF).
 */
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import { fileURLToPath } from "node:url";

/** "2.17" → [2, 17] */
const parse = (v) => v.split(".").map(Number);

/** a < b, for glibc versions. */
export function older(a, b) {
	const [x, y] = [parse(a), parse(b)];
	for (let i = 0; i < Math.max(x.length, y.length); i++) {
		if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) < (y[i] ?? 0);
	}
	return false;
}

/** The glibc versions in objdump's output, oldest first. */
export function glibcVersions(text) {
	const found = new Set([...text.matchAll(/\bGLIBC_(\d+(?:\.\d+)+)\b/g)].map((m) => m[1]));
	return [...found].sort((a, b) => (older(a, b) ? -1 : older(b, a) ? 1 : 0));
}

/** The newest glibc the binary needs, or undefined for one that needs none (a static build). */
export function glibcNeeded(file) {
	const dump = (flag) => execFileSync("objdump", [flag, file], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
	return glibcVersions(dump("-T") + dump("-p")).at(-1);
}

if (process.argv[1] && fs.realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
	const [file, max] = process.argv.slice(2);
	if (!file || !/^\d+\.\d+$/.test(max ?? "")) {
		console.error("usage: node scripts/check-glibc.mjs <binary> <glibc, e.g. 2.17>");
		process.exit(2);
	}
	try {
		const needed = glibcNeeded(file);
		if (needed && older(max, needed)) {
			console.error(`check-glibc: ${file} needs glibc ${needed}, newer than ${max}: it won't start on older systems`);
			process.exit(1);
		}
		console.log(`${file}: needs glibc ${needed ?? "(none)"}, runs on ${max} and newer`);
	} catch (err) {
		console.error(`check-glibc: ${err.message}`);
		process.exit(1);
	}
}
