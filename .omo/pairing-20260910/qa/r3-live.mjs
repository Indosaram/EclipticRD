import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import dgram from 'node:dgram';
import { EventEmitter } from 'node:events';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const root = process.cwd();
const binaries = join(root, 'clients/rust/target/debug');
const key = '52'.repeat(32);

function bounded(promise, milliseconds, label) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${label} timed out`)), milliseconds);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

function start(binary, args) {
  const child = spawn(join(binaries, binary), args, {
    cwd: root,
    detached: true,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, RUST_LOG: 'info', NO_COLOR: '1' },
  });
  const events = new EventEmitter();
  const record = { child, events, text: '' };
  for (const stream of [child.stdout, child.stderr]) {
    stream.on('data', chunk => {
      record.text += chunk.toString();
      events.emit('output');
    });
  }
  record.done = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => resolve({ code, signal }));
  });
  record.done.catch(() => {});
  return record;
}

async function ready(record) {
  const pattern = /QA_READY (\d+) (\d+)/;
  let listener;
  const observed = new Promise(resolve => {
    listener = () => {
      const match = pattern.exec(record.text);
      if (match) resolve({ tcp: Number(match[1]), udp: Number(match[2]) });
    };
    record.events.on('output', listener);
    listener();
  });
  try {
    return await bounded(Promise.race([
      observed,
      record.done.then(() => { throw new Error(`host exited before ready: ${record.text}`); }),
    ]), 15_000, 'host ready');
  } finally {
    record.events.off('output', listener);
  }
}

async function stop(record) {
  if (!record) return;
  try {
    process.kill(-record.child.pid, 'SIGTERM');
  } catch (error) {
    if (error.code !== 'ESRCH') throw error;
  }
  await bounded(record.done, 5000, 'process cleanup');
  try {
    process.kill(-record.child.pid, 0);
    throw new Error(`owned process group remains: ${record.child.pid}`);
  } catch (error) {
    if (error.code !== 'ESRCH') throw error;
  }
}

async function bindSocket(errors) {
  const socket = dgram.createSocket('udp4');
  socket.on('error', error => errors.push(String(error)));
  await bounded(new Promise((resolve, reject) => {
    socket.once('error', reject);
    socket.bind(0, '127.0.0.1', resolve);
  }), 3000, 'UDP bind');
  return socket;
}

function send(socket, payload, port) {
  return new Promise((resolve, reject) => {
    socket.send(payload, port, '127.0.0.1', error => error ? reject(error) : resolve());
  });
}

async function scenario(adversarial) {
  const directory = await mkdtemp(join(tmpdir(), 'erd-r3-qa-'));
  const errors = [];
  const sockets = [];
  let host;
  let client;
  let result;
  try {
    host = start('erd-pairing-qa-host', [join(directory, 'host.json')]);
    const ports = await ready(host);
    let udp = ports.udp;
    let roguePackets = 0;
    let rogueBytes = 0;
    let registrations = 0;
    if (adversarial) {
      const gate = await bindSocket(errors);
      sockets.push(gate);
      const rogue = await bindSocket(errors);
      sockets.push(rogue);
      udp = gate.address().port;
      let clientAddress;
      rogue.on('message', packet => {
        roguePackets += 1;
        rogueBytes += packet.length;
      });
      gate.on('message', (packet, address) => {
        (async () => {
          if (address.port === ports.udp) {
            if (!clientAddress) throw new Error('host media arrived before client registration');
            await send(gate, packet, clientAddress.port);
            return;
          }
          clientAddress = address;
          if (registrations++ === 0) {
            await send(rogue, Buffer.from([0x01, 0x02, 0x03]), ports.udp);
          }
          await send(gate, packet, ports.udp);
        })().catch(error => errors.push(String(error)));
      });
    }
    client = start('erd-client', [
      '--host', '127.0.0.1', '--tcp-port', String(ports.tcp),
      '--udp-port', String(udp), '--pairing-id', 'qa-r3-registration',
      '--psk-hex', key, '--frames', '1', '--timeout-secs', '5',
      '--client-name', 'isolated-r3-qa',
    ]);
    const exit = await bounded(client.done, 20_000, 'native client');
    assert.deepEqual(errors, [], 'QA UDP transport must not fail');
    result = {
      adversarial, ports, gatePort: udp, registrations, roguePackets, rogueBytes,
      exit, success: exit.code === 0 && /Reached requested target frame count/.test(client.text),
      clientOutput: client.text, hostOutput: host.text,
    };
    return result;
  } finally {
    const cleanup = await Promise.allSettled([
      ...sockets.map(socket => new Promise(resolve => socket.close(resolve))),
      stop(client),
      stop(host),
    ]);
    await rm(directory, { recursive: true, force: true });
    const cleanupErrors = cleanup
      .filter(entry => entry.status === 'rejected')
      .map(entry => String(entry.reason));
    console.log(`CLEANUP ${JSON.stringify({
      scenario: adversarial ? 'adversarial' : 'baseline',
      hostPid: host?.child.pid, clientPid: client?.child.pid,
      closedSockets: sockets.length, removedDirectory: directory, cleanupErrors,
    })}`);
    assert.deepEqual(cleanupErrors, [], 'all owned QA resources must be cleaned');
  }
}

const baseline = await scenario(false);
console.log(`R3_BASELINE ${JSON.stringify(baseline)}`);
assert.equal(baseline.success, true, 'real native baseline must decode a frame');
const attack = await scenario(true);
console.log(`R3_ADVERSARIAL ${JSON.stringify(attack)}`);
assert.equal(attack.registrations > 0, true, 'must observe the real client registration');
assert.equal(attack.roguePackets, 0, 'unauthenticated UDP sender must not receive media');
assert.equal(attack.success, true, 'legitimate native client must decode a frame');
console.log('R3_SURFACE_PASS');
