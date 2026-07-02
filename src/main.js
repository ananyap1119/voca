const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const statusDot = document.getElementById('status-dot');
const statusText = document.getElementById('status-text');
const hotkeyDisplay = document.getElementById('hotkey-display');
const languageSelect = document.getElementById('language');
const codemixCheck = document.getElementById('codemix');
const providerSelect = document.getElementById('provider');
const apiKeyEnvInput = document.getElementById('api-key-env');
const polishModeSelect = document.getElementById('polish-mode');
const polishEndpointInput = document.getElementById('polish-endpoint');
const polishModelInput = document.getElementById('polish-model');
const polishApiKeyEnvInput = document.getElementById('polish-api-key-env');
const configPath = document.getElementById('config-path');
const hotkeyStatus = document.getElementById('hotkey-status');
const testMicButton = document.getElementById('test-mic');
const dictateNowButton = document.getElementById('dictate-now');
const stopRecordingButton = document.getElementById('stop-recording');
const resetRecordingButton = document.getElementById('reset-recording');
const copyTranscriptButton = document.getElementById('copy-transcript');
const micStatus = document.getElementById('mic-status');
const micLevel = document.getElementById('mic-level');
const transcriptBox = document.getElementById('transcript');

let saveTimer = null;
let dictationTimer = null;
let dictationStartedAt = null;
let isDictating = false;

function displayHotkey(hotkey) {
  const isWindows = navigator.userAgent.toLowerCase().includes('windows');
  const isMac = navigator.platform.toLowerCase().includes('mac');
  if (!hotkey) return '';
  if (isWindows) return hotkey.replace('CmdOrCtrl+', 'Ctrl+').replaceAll('Cmd', 'Ctrl');
  if (isMac) return hotkey.replace('CmdOrCtrl+', 'Cmd+');
  return hotkey.replace('CmdOrCtrl+', 'Ctrl+');
}

function configPayload() {
  return {
    provider_name: providerSelect.value === 'local' ? 'local' : null,
    language: languageSelect.value,
    codemix: codemixCheck.checked,
    api_key_env_var: apiKeyEnvInput.value.trim() || null,
    polish_mode: polishModeSelect.value,
    polish_endpoint: polishEndpointInput.value.trim() || null,
    polish_model: polishModelInput.value.trim() || null,
    polish_api_key_env_var: polishApiKeyEnvInput.value.trim() || null,
  };
}

async function saveConfig() {
  try {
    await invoke('save_config', { config: configPayload() });
  } catch (e) {
    console.error('Failed to save config:', e);
  }
}

function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(saveConfig, 150);
}

function showMicStatus(message) {
  micStatus.textContent = message;
}

function setControlsEnabled(enabled) {
  dictateNowButton.disabled = !enabled;
  testMicButton.disabled = !enabled;
  dictateNowButton.style.opacity = enabled ? '1' : '0.55';
  testMicButton.style.opacity = enabled ? '1' : '0.55';
  dictateNowButton.style.cursor = enabled ? 'pointer' : 'not-allowed';
  testMicButton.style.cursor = enabled ? 'pointer' : 'not-allowed';
  stopRecordingButton.disabled = false;
  stopRecordingButton.style.opacity = isDictating ? '1' : '0.55';
  stopRecordingButton.style.cursor = 'pointer';
  resetRecordingButton.disabled = false;
  resetRecordingButton.style.opacity = '1';
  resetRecordingButton.style.cursor = 'pointer';
}

function startDictationUi() {
  clearInterval(dictationTimer);
  isDictating = true;
  dictationStartedAt = Date.now();
  setControlsEnabled(false);
  statusDot.className = 'status-dot recording';
  statusText.textContent = 'Recording... release hotkey or press Stop';
  showMicStatus('Recording dictation. Release the hotkey or press Stop when done.');
  micLevel.style.width = '4%';

  dictationTimer = setInterval(() => {
    const elapsed = Math.floor((Date.now() - dictationStartedAt) / 1000);
    statusText.textContent = `Recording... ${elapsed}s`;
    micLevel.style.width = `${Math.min(100, Math.max(4, (elapsed % 20) * 5))}%`;
  }, 250);
}

function stopDictationUi() {
  clearInterval(dictationTimer);
  dictationTimer = null;
  dictationStartedAt = null;
  isDictating = false;
  setControlsEnabled(true);
  micLevel.style.width = '0%';
}

async function copyTranscript() {
  const text = transcriptBox.value.trim();
  if (!text) {
    showMicStatus('No transcript yet');
    return;
  }

  try {
    await navigator.clipboard.writeText(text);
    showMicStatus('Transcript copied');
  } catch (e) {
    console.error('Failed to copy transcript:', e);
    showMicStatus('Copy failed');
  }
}

async function loadConfig() {
  try {
    const cfg = await invoke('get_config');
    const path = await invoke('get_config_path');
    const hotkeyMessage = await invoke('get_hotkey_status');
    configPath.textContent = path;
    providerSelect.value = cfg.provider_name === 'local' ? 'local' : 'saaras';
    if (cfg.language) languageSelect.value = cfg.language;
    if (cfg.codemix !== undefined) codemixCheck.checked = cfg.codemix;
    if (cfg.api_key_env_var) apiKeyEnvInput.value = cfg.api_key_env_var;
    polishModeSelect.value = cfg.polish_mode || 'light';
    polishEndpointInput.value = cfg.polish_endpoint || '';
    polishModelInput.value = cfg.polish_model || '';
    polishApiKeyEnvInput.value = cfg.polish_api_key_env_var || '';
    if (cfg.hotkey) hotkeyDisplay.textContent = displayHotkey(cfg.hotkey);
    statusText.innerHTML = `Ready - hold <span class="hotkey">${hotkeyDisplay.textContent}</span> to dictate`;
    hotkeyStatus.textContent = hotkeyMessage;
  } catch (e) {
    console.error('Failed to load config:', e);
    configPath.textContent = 'Failed to load';
  }
}

providerSelect.addEventListener('change', scheduleSave);
languageSelect.addEventListener('change', scheduleSave);
codemixCheck.addEventListener('change', scheduleSave);
apiKeyEnvInput.addEventListener('input', scheduleSave);
polishModeSelect.addEventListener('change', scheduleSave);
polishEndpointInput.addEventListener('input', scheduleSave);
polishModelInput.addEventListener('input', scheduleSave);
polishApiKeyEnvInput.addEventListener('input', scheduleSave);

void listen('dictation-started', async () => {
  startDictationUi();
});

void listen('dictation-status', async (event) => {
  showMicStatus(event.payload);
});

void listen('dictation-finished', async (event) => {
  stopDictationUi();
  statusDot.className = 'status-dot ready';
  const result = event.payload;
  const hotkey = hotkeyDisplay.textContent;
  const preview = result.text.substring(0, 40);
  statusText.innerHTML = `Ready with text: "${preview}${result.text.length > 40 ? '...' : ''}" - hold <span class="hotkey">${hotkey}</span> to dictate`;
  transcriptBox.value = result.text;
  transcriptBox.focus();
  transcriptBox.select();
  showMicStatus('Dictation complete');
});

void listen('dictation-error', async (event) => {
  stopDictationUi();
  statusDot.className = 'status-dot ready';
  statusText.textContent = `Error: ${event.payload}`;
  showMicStatus('Dictation failed');
});

void listen('hotkey-status', async (event) => {
  hotkeyStatus.textContent = event.payload;
});

testMicButton.addEventListener('click', async () => {
  if (isDictating) return;
  statusDot.className = 'status-dot recording';
  statusText.textContent = 'Testing mic...';
  showMicStatus('Requesting microphone permission...');
  micLevel.style.width = '0%';
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const audioContext = new AudioContext();
    const source = audioContext.createMediaStreamSource(stream);
    const analyser = audioContext.createAnalyser();
    analyser.fftSize = 256;
    source.connect(analyser);

    const data = new Uint8Array(analyser.fftSize);
    const startedAt = Date.now();

    const tick = () => {
      analyser.getByteTimeDomainData(data);
      let sum = 0;
      for (const value of data) {
        const centered = value - 128;
        sum += centered * centered;
      }
      const rms = Math.sqrt(sum / data.length) / 128;
      const level = Math.max(4, Math.min(100, Math.round(rms * 140)));
      micLevel.style.width = `${level}%`;
      showMicStatus(`Mic open: ${stream.getAudioTracks()[0]?.label || 'default input'} · level ${level}%`);

      if (Date.now() - startedAt < 5000) {
        requestAnimationFrame(tick);
      } else {
        stream.getTracks().forEach((track) => track.stop());
        audioContext.close();
        statusDot.className = 'status-dot ready';
        statusText.textContent = 'Mic test complete';
        showMicStatus('Mic stream closed');
        micLevel.style.width = '0%';
      }
    };

    tick();
  } catch (e) {
    statusDot.className = 'status-dot ready';
    statusText.textContent = `Error: ${e}`;
    showMicStatus('Mic test failed');
  }
});

testMicButton.addEventListener('pointerdown', () => {
  showMicStatus('Button pressed');
  testMicButton.textContent = 'Pressed';
  setTimeout(() => {
    testMicButton.textContent = 'Test mic';
  }, 300);
});

dictateNowButton.addEventListener('click', async () => {
  if (isDictating) {
    await invoke('stop_recording');
    showMicStatus('Stopping recording...');
    return;
  }

  startDictationUi();
  transcriptBox.value = '';

  try {
    const result = await invoke('toggle_dictation');
    statusDot.className = 'status-dot ready';
    const hotkey = hotkeyDisplay.textContent;
    const preview = result.text.substring(0, 40);
    statusText.innerHTML = `Ready with text: "${preview}${result.text.length > 40 ? '...' : ''}" - hold <span class="hotkey">${hotkey}</span> to dictate`;
    transcriptBox.value = result.text;
    transcriptBox.focus();
    transcriptBox.select();
    showMicStatus('Dictation complete');
  } catch (e) {
    stopDictationUi();
    statusDot.className = 'status-dot ready';
    statusText.textContent = `Error: ${e}`;
    showMicStatus('Dictation failed');
  }
});

stopRecordingButton.addEventListener('click', async () => {
  try {
    await invoke('stop_recording');
    showMicStatus('Stopping recording...');
  } catch (e) {
    statusText.textContent = `Error: ${e}`;
    showMicStatus('Stop failed');
  }
});

resetRecordingButton.addEventListener('click', async () => {
  try {
    await invoke('reset_recording');
    stopDictationUi();
    statusDot.className = 'status-dot ready';
    statusText.innerHTML = `Ready - hold <span class="hotkey">${hotkeyDisplay.textContent}</span> to dictate`;
    showMicStatus('Recording state reset');
  } catch (e) {
    statusDot.className = 'status-dot ready';
    statusText.textContent = `Error: ${e}`;
    showMicStatus('Reset failed');
  }
});

copyTranscriptButton.addEventListener('click', copyTranscript);

loadConfig();

setTimeout(() => {
  invoke('reveal_window').catch((e) => {
    console.error('Failed to reveal window:', e);
  });
}, 8000);
