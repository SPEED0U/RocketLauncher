#!/usr/bin/env node

import { spawn, spawnSync } from 'child_process'; // spawnSync used for cargo update
import { readFileSync, writeFileSync, readdirSync, statSync, existsSync, mkdirSync, copyFileSync, unlinkSync } from 'fs';
import { join, dirname, basename } from 'path';
import { fileURLToPath } from 'url';
import readline from 'readline';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const rootDir = join(__dirname, '..');
const tauriDir = join(rootDir, 'src-tauri');

// Get arguments passed to the script
const args = process.argv.slice(2);
const isFinal = args.includes('-final') || args.includes('--final');

// Remove -final from args to pass clean args to tauri
const tauriArgs = args.filter(arg => arg !== '-final' && arg !== '--final');

// Convert a Windows absolute path to its WSL /mnt/... equivalent
function windowsPathToWsl(winPath) {
  return winPath
    .replace(/\\/g, '/')
    .replace(/^([A-Za-z]):/, (_, d) => `/mnt/${d.toLowerCase()}`);
}

// Check whether WSL is usable on this machine
function isWslAvailable() {
  try {
    const result = spawnSync('wsl', ['echo', 'ok'], { shell: true, stdio: 'pipe', timeout: 8000 });
    return result.status === 0;
  } catch {
    return false;
  }
}

async function askVersion() {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
  });

  return new Promise((resolve) => {
    rl.question('Enter version number (e.g., 1.0.0): ', (version) => {
      rl.close();
      resolve(version.trim());
    });
  });
}

function updateVersion(version) {
  console.log(`\nUpdating version to ${version}...\n`);

  // Update tauri.conf.json
  const tauriConfPath = join(tauriDir, 'tauri.conf.json');
  const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'));
  tauriConf.version = version;
  writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + '\n', 'utf8');
  console.log('Updated tauri.conf.json');

  // Update Cargo.toml
  const cargoTomlPath = join(tauriDir, 'Cargo.toml');
  let cargoToml = readFileSync(cargoTomlPath, 'utf8');
  cargoToml = cargoToml.replace(/^version = "[^"]+"/m, `version = "${version}"`);
  writeFileSync(cargoTomlPath, cargoToml, 'utf8');
  console.log('Updated Cargo.toml');

  // Update package.json
  const packageJsonPath = join(rootDir, 'package.json');
  const packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf8'));
  packageJson.version = version;
  writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2) + '\n', 'utf8');
  console.log('Updated package.json');
  
  // Update Cargo.lock to reflect the new version
  console.log('Updating Cargo.lock...');
  const proc = spawnSync('cargo', ['update', '-p', 'rocket-launcher'], {
    cwd: tauriDir,
    shell: true,
    stdio: 'inherit'
  });
  
  if (proc.status === 0) {
    console.log('Updated Cargo.lock\n');
  } else {
    console.warn('Warning: Could not update Cargo.lock (this may affect version naming)\n');
  }
}

// Windows build (on host)
function runWindowsBuild(extraArgs = []) {
  return new Promise((resolve, reject) => {
    console.log('\n[Windows] Building Tauri app...\n');
    const cmd = ['npx', 'tauri', 'build', ...extraArgs].join(' ');
    const proc = spawn(cmd, [], {
      cwd: rootDir,
      shell: true,
      stdio: 'inherit'
    });
    proc.on('close', (code) => {
      if (code !== 0) reject(new Error(`Windows build failed with code ${code}`));
      else resolve();
    });
    proc.on('error', reject);
  });
}

// Linux build via WSL — uses Ubuntu-22.04 for GLIBC 2.35 compatibility
// Falls back to the default distro if Ubuntu-22.04 is not installed
function getWslDistro() {
  try {
    const result = spawnSync('wsl', ['-l', '-q'], { shell: false, stdio: 'pipe', timeout: 8000, encoding: 'utf16le' });
    const distros = result.stdout.split(/\r?\n/).map(l => l.trim().replace(/\x00/g, '')).filter(Boolean);
    if (distros.includes('Ubuntu-22.04')) return 'Ubuntu-22.04';
  } catch { /* ignore */ }
  return null;
}

function runLinuxBuild(version, extraArgs = []) {
  return new Promise((resolve, reject) => {
    console.log('\n[Linux] Building Tauri app via WSL...\n');
    const wslPath = windowsPathToWsl(rootDir);

    // Write the bash script to a file to avoid all template/escaping issues
    const scriptPath = join(rootDir, 'scripts', '_linux-build.sh');
    const scriptContent = [
      '#!/usr/bin/env bash',
      'set -e',
      '',
      '# Load Rust',
      'source "$HOME/.cargo/env" 2>/dev/null || true',
      '',
      '# Load nvm',
      'NVM_SCRIPT=""',
      'for candidate in "$HOME/.nvm/nvm.sh" "/home/$(whoami)/.nvm/nvm.sh"; do',
      '  [ -s "$candidate" ] && NVM_SCRIPT="$candidate" && break',
      'done',
      'if [ -n "$NVM_SCRIPT" ]; then',
      '  export NVM_DIR="$(dirname "$NVM_SCRIPT")"',
      '  . "$NVM_SCRIPT"',
      'fi',
      '',
      '# Ensure Node >= 18',
      'NODE_MAJOR=$(node --version 2>/dev/null | cut -d. -f1 | tr -d "v" || echo "0")',
      'if [ "$NODE_MAJOR" -lt 18 ]; then',
      '  echo "[Linux] Node $NODE_MAJOR too old, installing Node 20 via nvm..."',
      '  if [ -z "$NVM_SCRIPT" ]; then',
      '    curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash',
      '    export NVM_DIR="$HOME/.nvm"',
      '    . "$NVM_DIR/nvm.sh"',
      '  fi',
      '  nvm install 20',
      '  nvm use 20',
      'fi',
      '',
      'echo "[Linux] Node $(node --version), npm $(npm --version)"',
      '',
      `cd "${wslPath}"`,
      'npm install --silent',
      '',
      '# Use a native Linux target dir to avoid GLIBC mismatch with Windows-compiled build scripts',
      'export CARGO_TARGET_DIR="/tmp/rocket-launcher-target"',
      'mkdir -p "$CARGO_TARGET_DIR"',
      '',
      '# Skip beforeBuildCommand: out/ already built by Windows step, NTFS write fails from WSL',
      'cp src-tauri/tauri.conf.json src-tauri/tauri.conf.json.bak',
      'trap \'mv src-tauri/tauri.conf.json.bak src-tauri/tauri.conf.json 2>/dev/null; exit 1\' ERR',
      'node -e "const fs=require(\'fs\'); const c=JSON.parse(fs.readFileSync(\'src-tauri/tauri.conf.json\',\'utf8\')); delete c.build.beforeBuildCommand; c.bundle.targets=[\'deb\']; fs.writeFileSync(\'src-tauri/tauri.conf.json\', JSON.stringify(c,null,2));"',
      '',
      '# Build .deb via Tauri',
      `CARGO_TARGET_DIR=/tmp/rocket-launcher-target npx tauri build --bundles deb ${extraArgs.join(' ')}`.trimEnd(),
      '',
      'mv src-tauri/tauri.conf.json.bak src-tauri/tauri.conf.json',
      '',
      'BINARY="/tmp/rocket-launcher-target/release/rocket-launcher"',
      `VERSION="${version}"`,
      'PKGDIR="/tmp/rocket-launcher-pkg"',
      '',
      '# Build .deb with dpkg --build (only the binary, no bundled libs)',
      'rm -rf "$PKGDIR"',
      'mkdir -p "$PKGDIR/DEBIAN"',
      'mkdir -p "$PKGDIR/usr/bin"',
      'mkdir -p "$PKGDIR/usr/share/applications"',
      'mkdir -p "$PKGDIR/usr/share/icons/hicolor/128x128/apps"',
      '',
      'cp "$BINARY" "$PKGDIR/usr/bin/rocket-launcher"',
      'chmod 755 "$PKGDIR/usr/bin/rocket-launcher"',
      `cp "${wslPath}/src-tauri/icons/128x128.png" "$PKGDIR/usr/share/icons/hicolor/128x128/apps/rocket-launcher.png"`,
      '',
      'cat > "$PKGDIR/usr/share/applications/rocket-launcher.desktop" << \'DESKTOP\'',
      '[Desktop Entry]',
      'Name=RocketLauncher',
      'Exec=rocket-launcher',
      'Icon=rocket-launcher',
      'Type=Application',
      'Categories=Game;',
      'StartupNotify=true',
      'DESKTOP',
      '',
      'INSTALLED_SIZE=$(du -sk "$PKGDIR/usr" | awk \'{print $1}\')',
      '',
      'cat > "$PKGDIR/DEBIAN/control" << EOF',
      'Package: rocket-launcher',
      'Version: $VERSION',
      'Architecture: amd64',
      'Maintainer: speedou',
      'Installed-Size: $INSTALLED_SIZE',
      'Section: games',
      'Priority: optional',
      'Description: Rocket Launcher for Need for Speed: World',
      'EOF',
      '',
      `DEB_OUT="/tmp/rocket-launcher-target/release/bundle/deb/RocketLauncher_${version}_amd64.deb"`,
      'mkdir -p "$(dirname "$DEB_OUT")"',
      'dpkg --build "$PKGDIR" "$DEB_OUT"',
      'echo "[Linux] .deb created: $DEB_OUT"',
      '',
      '# Copy Linux artifacts back to NTFS so Windows build script can find them',
      `NTFS_BUNDLE="${wslPath}/src-tauri/target/release/bundle"`,
      'mkdir -p "$NTFS_BUNDLE/appimage" "$NTFS_BUNDLE/deb"',
      'cp "$DEB_OUT" "$NTFS_BUNDLE/deb/" 2>/dev/null || true',
      `echo "[Linux] Artifacts copied to ${wslPath}/src-tauri/target/release/bundle/"`,
    ].join('\n');

    writeFileSync(scriptPath, scriptContent, { encoding: 'utf8', mode: 0o755 });

    const wslScriptPath = windowsPathToWsl(scriptPath);
    const distro = getWslDistro();
    const wslArgs = distro
      ? ['-d', distro, '--', 'bash', wslScriptPath]
      : ['--', 'bash', wslScriptPath];

    if (distro) {
      console.log(`[Linux] Using WSL distro: ${distro}\n`);
    } else {
      console.warn('[Linux] Ubuntu-22.04 not found, using default WSL distro\n');
    }

    const proc = spawn('wsl', wslArgs, {
      cwd: rootDir,
      shell: false,
      stdio: 'inherit'
    });
    proc.on('close', (code) => {
      try { unlinkSync(scriptPath); } catch { /* ignore */ }
      if (code !== 0) reject(new Error(`Linux build failed with code ${code}`));
      else resolve();
    });
    proc.on('error', (err) => {
      try { unlinkSync(scriptPath); } catch { /* ignore */ }
      reject(err);
    });
  });
}

function findExeFile(bundleDir, version) {
  const nsisDir = join(bundleDir, 'nsis');

  if (!existsSync(nsisDir)) {
    throw new Error('NSIS directory not found in bundle output');
  }

  const files = readdirSync(nsisDir);
  // Prefer exe matching the current version, fallback to any setup exe
  const exeFile =
    files.find(f => f.endsWith('.exe') && !f.includes('uninstall') && f.includes(version)) ||
    files.find(f => f.endsWith('.exe') && !f.includes('uninstall'));

  if (!exeFile) {
    throw new Error('Setup .exe file not found in NSIS output');
  }

  return join(nsisDir, exeFile);
}

function findLinuxArtifacts(bundleDir, version) {
  const result = {};

  const appimageDir = join(bundleDir, 'appimage');
  if (existsSync(appimageDir)) {
    const files = readdirSync(appimageDir);
    const f =
      files.find(f => f.endsWith('.AppImage') && f.includes(version)) ||
      files.find(f => f.endsWith('.AppImage'));
    if (f) result.appimage = join(appimageDir, f);
  }

  const debDir = join(bundleDir, 'deb');
  if (existsSync(debDir)) {
    const files = readdirSync(debDir);
    const f =
      files.find(f => f.endsWith('.deb') && f.includes(version)) ||
      files.find(f => f.endsWith('.deb'));
    if (f) result.deb = join(debDir, f);
  }

  return result;
}

function createLatestJson(version, winExePath, linuxArtifacts) {
  const platforms = {};

  if (winExePath) {
    platforms.windows = { exe: basename(winExePath) };
  }

  if (linuxArtifacts?.appimage || linuxArtifacts?.deb) {
    platforms.linux = {};
    if (linuxArtifacts.appimage) platforms.linux.appimage = basename(linuxArtifacts.appimage);
    if (linuxArtifacts.deb) platforms.linux.deb = basename(linuxArtifacts.deb);
  }

  const latestData = {
    version,
    publishDate: new Date().toISOString(),
    productName: "RocketLauncher",
    platforms
  };

  // All artifacts + latest.json go into dist/<version>/
  const distDir = join(rootDir, 'dist', version);
  mkdirSync(distDir, { recursive: true });

  // Copy Windows artifact
  if (winExePath) {
    const dest = join(distDir, basename(winExePath));
    copyFileSync(winExePath, dest);
    console.log(`Copied ${basename(winExePath)} → dist/${version}/`);
  }

  // Copy Linux artifacts
  if (linuxArtifacts?.appimage) {
    const dest = join(distDir, basename(linuxArtifacts.appimage));
    copyFileSync(linuxArtifacts.appimage, dest);
    console.log(`Copied ${basename(linuxArtifacts.appimage)} → dist/${version}/`);
  }
  if (linuxArtifacts?.deb) {
    const dest = join(distDir, basename(linuxArtifacts.deb));
    copyFileSync(linuxArtifacts.deb, dest);
    console.log(`Copied ${basename(linuxArtifacts.deb)} → dist/${version}/`);
  }

  const latestJsonPath = join(distDir, 'latest.json');
  writeFileSync(latestJsonPath, JSON.stringify(latestData, null, 2) + '\n', 'utf8');

  console.log('\nCreated latest.json:');
  console.log(JSON.stringify(latestData, null, 2));
  console.log(`\nAll artifacts in: dist/${version}/\n`);

  return latestJsonPath;
}

async function main() {
  try {
    let version = null;

    if (isFinal) {
      console.log('Building FINAL release\n');
      version = await askVersion();
      
      if (!version || !/^\d+\.\d+\.\d+/.test(version)) {
        console.error('Invalid version format. Expected: x.y.z');
        process.exit(1);
      }

      updateVersion(version);

      // --- Windows build ---
      await runWindowsBuild(tauriArgs);

      // --- Linux build via WSL ---
      if (isWslAvailable()) {
        try {
          await runLinuxBuild(version, tauriArgs);
        } catch (e) {
          console.error(`\n[Linux] Build failed: ${e.message}`);
          console.error('Linux artifacts will be missing from the release.\n');
          process.exit(1);
        }
      } else {
        console.warn('\nWSL not available — skipping Linux build.\n');
      }

      // --- Collect artifacts and generate latest.json ---
      const bundleDir = join(tauriDir, 'target', 'release', 'bundle');

      let winExePath = null;
      try {
        winExePath = findExeFile(bundleDir, version);
      } catch (e) {
        console.warn(`Warning: Windows artifact not found: ${e.message}`);
      }

      const linuxArtifacts = findLinuxArtifacts(bundleDir, version);

      createLatestJson(version, winExePath, linuxArtifacts);

      console.log('Final release build complete!\n');
    } else {
      const tauriConfPath = join(tauriDir, 'tauri.conf.json');
      const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'));
      version = tauriConf.version;
      console.log(`Building version ${version}...\n`);

      // Run build with remaining args
      await runWindowsBuild(tauriArgs);

      console.log('\nBuild complete!\n');
    }

  } catch (error) {
    console.error('\nBuild failed:', error.message);
    process.exit(1);
  }
}

main();
