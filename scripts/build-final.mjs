#!/usr/bin/env node

import { spawn } from 'child_process';
import { readFileSync, writeFileSync, readdirSync, statSync } from 'fs';
import { join, dirname, basename } from 'path';
import { fileURLToPath } from 'url';
import readline from 'readline';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const rootDir = join(__dirname, '..');
const tauriDir = join(rootDir, 'src-tauri');

// Check if -final argument is present
const isFinal = process.argv.includes('-final');

async function askVersion() {
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
  });

  return new Promise((resolve) => {
    rl.question('Enter version number (ex: 1.0.0 or 1.0.0b): ', (version) => {
      rl.close();
      resolve(version.trim());
    });
  });
}

function isBetaVersion(version) {
  return version.endsWith('b');
}

function manifestVersion(version) {
  return isBetaVersion(version) ? `${version.slice(0, -1)}-beta.0` : version;
}

function updateVersion(version) {
  console.log(`\npdating version to ${version}...\n`);
  const cargoVersion = manifestVersion(version);

  // Update tauri.conf.json
  const tauriConfPath = join(tauriDir, 'tauri.conf.json');
  const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'));
  tauriConf.version = cargoVersion;
  writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + '\n', 'utf8');
  console.log('Updated tauri.conf.json');

  // Update Cargo.toml
  const cargoTomlPath = join(tauriDir, 'Cargo.toml');
  let cargoToml = readFileSync(cargoTomlPath, 'utf8');
  cargoToml = cargoToml.replace(/version = "[^"]+"/g, `version = "${cargoVersion}"`);
  writeFileSync(cargoTomlPath, cargoToml, 'utf8');
  console.log('Updated Cargo.toml');

  // Update package.json
  const packageJsonPath = join(rootDir, 'package.json');
  const packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf8'));
  packageJson.version = cargoVersion;
  writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2) + '\n', 'utf8');
  console.log('Updated package.json\n');
}

function runBuild(version) {
  return new Promise((resolve, reject) => {
    console.log('Building app...\n');

    const proc = spawn('npx', ['tauri', 'build'], {
      cwd: rootDir,
      shell: true,
      stdio: 'inherit',
      env: {
        ...process.env,
        ROCKET_LAUNCHER_VERSION: version,
        ROCKET_LAUNCHER_CHANNEL: isBetaVersion(version) ? 'beta' : 'stable'
      }
    });

    proc.on('close', (code) => {
      if (code !== 0) {
        reject(new Error(`Build failed with code ${code}`));
      } else {
        resolve();
      }
    });

    proc.on('error', reject);
  });
}

function findExeFile(bundleDir) {
  // Look for NSIS output directory
  const nsisDir = join(bundleDir, 'nsis');
  
  if (!statSync(nsisDir, { throwIfNoEntry: false })) {
    throw new Error('NSIS directory not found in bundle output');
  }

  const files = readdirSync(nsisDir);
  const exeFile = files.find(f => f.endsWith('.exe') && !f.includes('_uninstall'));

  if (!exeFile) {
    throw new Error('Setup .exe file not found in NSIS output');
  }

  return join(nsisDir, exeFile);
}

function createReleaseJson(version, exePath) {
  const exeName = basename(exePath);
  const jsonName = isBetaVersion(version) ? 'beta.json' : 'latest.json';
  const latestJsonPath = join(dirname(exePath), jsonName);

  const latestData = {
    version: version,
    exe: exeName,
    absolutePath: exePath.replace(/\\/g, '/'), // Normalize path separators
    publishDate: new Date().toISOString(),
    productName: "Rocket Launcher"
  };

  writeFileSync(latestJsonPath, JSON.stringify(latestData, null, 2) + '\n', 'utf8');
  
  console.log(`\nCreated ${jsonName}:`);
  console.log(JSON.stringify(latestData, null, 2));
  console.log(`\nLocation: ${latestJsonPath}\n`);
}

async function main() {
  try {
    let version = null;

    if (isFinal) {
      console.log('Building FINAL release\n');
      version = await askVersion();
      
      if (!version || !/^\d+\.\d+\.\d+b?$/.test(version)) {
        console.error('Invalid version format. Expected: x.y.z or x.y.zb');
        process.exit(1);
      }

      updateVersion(version);
    } else {
      // Read current version for non-final builds
      const tauriConfPath = join(tauriDir, 'tauri.conf.json');
      const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'));
      version = tauriConf.version;
      console.log(`Building version ${version}...\n`);
    }

    // Run build
    await runBuild(version);

    if (isFinal) {
      console.log('\ninalizing release...\n');

      // Find the bundle output directory
      const bundleDir = join(tauriDir, 'target', 'release', 'bundle');
      const exePath = findExeFile(bundleDir);

      // Create the channel-specific update file
      createReleaseJson(version, exePath);

      console.log('Final release build complete!\n');
    } else {
      console.log('\nBuild complete!\n');
    }

  } catch (error) {
    console.error('\nBuild failed:', error.message);
    process.exit(1);
  }
}

main();
