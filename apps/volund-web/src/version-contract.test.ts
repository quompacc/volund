import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("Produktversionsvertrag", () => {
  it("hält Rust, npm, CMake und die Backup-Unit auf derselben Releaseversion", () => {
    const cargo = readFileSync(new URL("../../../Cargo.toml", import.meta.url), "utf8");
    const lock = readFileSync(new URL("../../../Cargo.lock", import.meta.url), "utf8");
    const cmake = readFileSync(
      new URL("../../../native/cad-convert/CMakeLists.txt", import.meta.url),
      "utf8",
    );
    const backupUnit = readFileSync(
      new URL("../../../deploy/systemd/volund-backup.service.example", import.meta.url),
      "utf8",
    );
    const backupTest = readFileSync(
      new URL("../../../deploy/backup/tests/backup-test.sh", import.meta.url),
      "utf8",
    );
    const installChecks = ["first-install.py", "provision-runtime.py", "upgrade-rehearsal.py"].map(
      (name) =>
        readFileSync(new URL(`../../../deploy/debian/tests/${name}`, import.meta.url), "utf8"),
    );
    const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
    const packageLock = JSON.parse(
      readFileSync(new URL("../package-lock.json", import.meta.url), "utf8"),
    );

    expect(pkg.version).toBe("0.39.0");
    expect(packageLock.version).toBe(pkg.version);
    expect(packageLock.packages[""].version).toBe(pkg.version);
    expect(cargo).toContain(`version = "${pkg.version}"`);
    expect(lock.match(/name = "volund(?:-core|d)"\nversion = "([^"]+)"/g)).toEqual([
      `name = "volund-core"\nversion = "${pkg.version}"`,
      `name = "volundd"\nversion = "${pkg.version}"`,
    ]);
    expect(cmake).toContain(`project(volund-cad-convert VERSION ${pkg.version} LANGUAGES CXX)`);
    expect(backupUnit).toContain(`Environment=VOLUND_VERSION=${pkg.version}`);
    expect(backupTest).toContain(`VOLUND_VERSION=${pkg.version}`);
    expect(backupTest).toContain(`'"version":"${pkg.version}"'`);
    for (const check of installChecks) {
      expect(check).toContain(`'version': '${pkg.version}'`);
    }
  });
});
