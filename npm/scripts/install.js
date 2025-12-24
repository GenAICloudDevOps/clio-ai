const https = require('https');
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const VERSION = '0.1.0';
const REPO = 'your-username/clio-ai'; // Update this

const PLATFORMS = {
  'darwin-x64': 'clio-ai-darwin-x64',
  'darwin-arm64': 'clio-ai-darwin-arm64',
  'linux-x64': 'clio-ai-linux-x64',
  'linux-arm64': 'clio-ai-linux-arm64',
  'win32-x64': 'clio-ai-windows-x64.exe',
};

const platform = `${process.platform}-${process.arch}`;
const binary = PLATFORMS[platform];

if (!binary) {
  console.error(`Unsupported platform: ${platform}`);
  process.exit(1);
}

const binDir = path.join(__dirname, '..', 'bin');
const binPath = path.join(binDir, process.platform === 'win32' ? 'clio-ai.exe' : 'clio-ai');

// For local dev, just create a wrapper that runs cargo
if (!fs.existsSync(binDir)) {
  fs.mkdirSync(binDir, { recursive: true });
}

// Create wrapper script for development
const wrapper = process.platform === 'win32' 
  ? `@echo off\ncargo run --manifest-path "${path.join(__dirname, '..', '..', 'Cargo.toml')}" -- %*`
  : `#!/bin/sh\ncargo run --manifest-path "${path.join(__dirname, '..', '..', 'Cargo.toml')}" -- "$@"`;

fs.writeFileSync(binPath, wrapper);
if (process.platform !== 'win32') {
  fs.chmodSync(binPath, '755');
}

console.log('clio-ai installed (dev mode - using cargo run)');
