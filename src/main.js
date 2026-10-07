const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const statusDot = document.getElementById('status-dot');
const statusText = document.getElementById('status-text');
const hotkeyDisplay = document.getElementById('hotkey-display');
const languageSelect = document.getElementById('language');
const codemixCheck = document.getElementById('codemix');
const providerSelect = document.getElementById('provider');
const apiKeyInput = document.getElementById('api-key');
const saveApiKeyButton = document.getElementById('save-api-key');
const removeApiKeyButton = document.getElementById('remove-api-key');
const apiKeyStatus = document.getElementById('api-key-status');
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
const recordingIndicator = document.getElementById('recording-indicator');
const rawTranscriptBox = document.getElementById('raw-transcript');
const finalTranscriptBox = document.getElementById('final-transcript');
const diagnosticsBox = document.getElementById('diagnostics');
const normalResults = document.getElementById('normal-results');
const evaluationModeCheck = document.getElementById('evaluation-mode');
const expectedTextRow = document.getElementById('expected-text-row');
const expectedTextInput = document.getElementById('expected-text');
const evaluationResults = document.getElementById('evaluation-results');
const evaluationV3Transcript = document.getElementById('evaluation-v3-transcript');
const evaluationV4Transcript = document.getElementById('evaluation-v4-transcript');
const evaluationV3Meta = document.getElementById('evaluation-v3-meta');
const evaluationV4Meta = document.getElementById('evaluation-v4-meta');
const evaluationComparison = document.getElementById('evaluation-comparison');
const evaluationHistory = document.getElementById('evaluation-history');
const evaluationStatus = document.getElementById('evaluation-status');
const refreshEvaluationButton = document.getElementById('refresh-evaluation');
const exportEvaluationButton = document.getElementById('export-evaluation');
const clearEvaluationButton = document.getElementById('clear-evaluation');

let saveTimer = null;
let evaluationSaveTimer = null;
let isDictating = false;
let evaluationRunReceived = false;
let latestEvaluationRunNumber = 0;

function displayHotkey(hotkey) {
  const isWindows = navigator.userAgent.toLowerCase().includes('windows');
  const isMac = navigator.platform.toLowerCase().includes('mac');
  if (!hotkey) return '';
  if (hotkey.toLowerCase() === 'rightalt') return 'Right Alt';
  if (isWindows) return hotkey.replace('CmdOrCtrl+', 'Ctrl+').replaceAll('Cmd', 'Ctrl');
  if (isMac) return hotkey.replace('CmdOrCtrl+', 'Cmd+');
  return hotkey.replace('CmdOrCtrl+', 'Ctrl+');
}

function configPayload() {
  return {
    provider_name: providerSelect.value === 'local' ? 'local' : null,
    language: languageSelect.value,
    codemix: codemixCheck.checked,
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
  isDictating = true;
  evaluationRunReceived = false;
  setControlsEnabled(false);
  statusDot.className = 'status-dot recording';
  statusText.textContent = 'Recording... release hotkey or press Stop';
  showMicStatus('Recording dictation. Release the hotkey or press Stop when done.');
  recordingIndicator.textContent = 'Recording...';
}

function stopDictationUi() {
  isDictating = false;
  setControlsEnabled(true);
  recordingIndicator.textContent = 'Not recording';
}

async function copyTranscript() {
  const text = finalTranscriptBox.value;
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

function renderDictationResult(result) {
  rawTranscriptBox.value = result.raw_transcript ?? '';
  finalTranscriptBox.value = result.final_transcript ?? '';

  const diagnostics = result.diagnostics;
  if (!diagnostics) {
    diagnosticsBox.innerHTML = '<span>Diagnostics unavailable</span>';
    return;
  }

  const values = [
    ['Audio', `${diagnostics.duration_ms} ms`],
    ['Chunks', diagnostics.chunk_count],
    ['Sarvam', `${diagnostics.sarvam_request_ms} ms`],
    ['Postprocess', `${diagnostics.postprocess_ms} ms`],
    ['Paste', `${diagnostics.paste_ms} ms`],
    ['After recording', `${diagnostics.total_after_recording_ms} ms`],
    ['Selected language', diagnostics.selected_language_code],
    ['Returned language', diagnostics.returned_language_code || 'not returned'],
    ['Codemix', diagnostics.codemix ? 'enabled' : 'disabled'],
    ['Model', diagnostics.model],
    ['Audio format', `${diagnostics.sample_rate} Hz / ${diagnostics.channel_count} ch`],
    ['Peak level', `${diagnostics.peak_level_percent.toFixed(1)}%`],
    ['Insertion', diagnostics.insertion_succeeded ? 'succeeded' : 'failed'],
  ];
  diagnosticsBox.replaceChildren(...values.map(([label, value]) => {
    const item = document.createElement('span');
    item.textContent = `${label}: ${value}`;
    return item;
  }));

  const preview = result.final_transcript.substring(0, 40);
  statusText.textContent = `Transcript ready: "${preview}${result.final_transcript.length > 40 ? '...' : ''}" - hold ${hotkeyDisplay.textContent} to dictate`;
}

function modelTranscript(modelResult) {
  if (modelResult.success) return modelResult.raw_transcript ?? '';
  return `ERROR: ${modelResult.error ?? 'Unknown error'}`;
}

function modelMeta(modelResult) {
  const language = modelResult.returned_language_code || 'not returned';
  const probability = modelResult.language_probability == null
    ? 'not returned'
    : modelResult.language_probability;
  return `Language returned: ${language}\nLanguage probability: ${probability}\nRequest latency: ${modelResult.request_latency_ms} ms`;
}

function renderEvaluationRun(run) {
  evaluationRunReceived = true;
  latestEvaluationRunNumber = run.run_number;
  evaluationResults.hidden = false;
  evaluationV3Transcript.value = modelTranscript(run.v3);
  evaluationV4Transcript.value = modelTranscript(run.v4);
  evaluationV3Meta.textContent = modelMeta(run.v3);
  evaluationV4Meta.textContent = modelMeta(run.v4);
  const latencyDifference = run.v4.request_latency_ms - run.v3.request_latency_ms;
  const values = [
    ['V3 latency', `${run.v3.request_latency_ms} ms`],
    ['V4 latency', `${run.v4.request_latency_ms} ms`],
    ['Difference (V4 - V3)', `${latencyDifference} ms`],
    ['Request order', run.request_order.join(' then ')],
  ];
  evaluationComparison.replaceChildren(...values.map(([label, value]) => {
    const item = document.createElement('span');
    item.textContent = `${label}: ${value}`;
    return item;
  }));
  evaluationStatus.textContent = `Evaluation run ${run.run_number} complete. No transcript was pasted.`;
}

function renderEvaluationHistory(runs) {
  evaluationHistory.replaceChildren(...runs.map((run) => {
    const row = document.createElement('tr');
    const values = [
      run.run_number,
      run.expected_text || '',
      modelTranscript(run.v3),
      modelTranscript(run.v4),
      run.v3.request_latency_ms,
      run.v4.request_latency_ms,
    ];
    row.replaceChildren(...values.map((value) => {
      const cell = document.createElement('td');
      cell.textContent = value;
      return cell;
    }));
    return row;
  }));
}

async function refreshEvaluationResults() {
  if (!evaluationModeCheck.checked) return;

  try {
    const session = await invoke('get_evaluation_session');
    const runs = session.runs || [];
    const latestRun = runs.length > 0 ? runs[runs.length - 1] : null;
    if (latestRun && latestRun.run_number !== latestEvaluationRunNumber) {
      renderEvaluationRun(latestRun);
    }
    renderEvaluationHistory(runs);
  } catch (e) {
    evaluationStatus.textContent = `Could not refresh evaluation results: ${e}`;
  }
}

function applyEvaluationMode(enabled) {
  expectedTextRow.hidden = !enabled;
  normalResults.hidden = enabled;
  if (!enabled) evaluationResults.hidden = true;
  evaluationStatus.textContent = enabled
    ? 'Evaluation enabled. Hold Right Alt to record one V3-vs-V4 comparison.'
    : 'Evaluation mode is off. No results are persisted automatically.';
}

async function saveEvaluationSettings() {
  try {
    await invoke('set_evaluation_settings', {
      enabled: evaluationModeCheck.checked,
      expectedText: expectedTextInput.value,
    });
  } catch (e) {
    evaluationStatus.textContent = `Could not update evaluation mode: ${e}`;
  }
}

function scheduleEvaluationSettingsSave() {
  clearTimeout(evaluationSaveTimer);
  evaluationSaveTimer = setTimeout(saveEvaluationSettings, 100);
}

async function loadConfig() {
  try {
    const cfg = await invoke('get_config');
    const path = await invoke('get_config_path');
    const hotkeyMessage = await invoke('get_hotkey_status');
    configPath.textContent = path;
    providerSelect.value = 'saaras';
    if (cfg.language) languageSelect.value = cfg.language;
    if (cfg.codemix !== undefined) codemixCheck.checked = cfg.codemix;
    const keySaved = await invoke('has_api_key');
    apiKeyStatus.textContent = keySaved
      ? 'API key saved securely on this computer'
      : 'Add your Sarvam API key before dictating';
    polishModeSelect.value = cfg.polish_mode || 'off';
    polishEndpointInput.value = cfg.polish_endpoint || '';
    polishModelInput.value = cfg.polish_model || '';
    polishApiKeyEnvInput.value = cfg.polish_api_key_env_var || '';
    if (cfg.hotkey) hotkeyDisplay.textContent = displayHotkey(cfg.hotkey);
    statusText.innerHTML = `Ready - hold <span class="hotkey">${hotkeyDisplay.textContent}</span> to dictate`;
    hotkeyStatus.textContent = hotkeyMessage;
    const evaluationSession = await invoke('get_evaluation_session');
    evaluationModeCheck.checked = evaluationSession.enabled;
    expectedTextInput.value = evaluationSession.expected_text || '';
    applyEvaluationMode(evaluationSession.enabled);
    const runs = evaluationSession.runs || [];
    const latestRun = runs.length > 0 ? runs[runs.length - 1] : null;
    if (latestRun) renderEvaluationRun(latestRun);
    renderEvaluationHistory(runs);
  } catch (e) {
    console.error('Failed to load config:', e);
    configPath.textContent = 'Failed to load';
  }
}

providerSelect.addEventListener('change', scheduleSave);
languageSelect.addEventListener('change', scheduleSave);
codemixCheck.addEventListener('change', scheduleSave);
polishModeSelect.addEventListener('change', scheduleSave);
polishEndpointInput.addEventListener('input', scheduleSave);
polishModelInput.addEventListener('input', scheduleSave);
polishApiKeyEnvInput.addEventListener('input', scheduleSave);

saveApiKeyButton.addEventListener('click', async () => {
  const apiKey = apiKeyInput.value.trim();
  if (!apiKey) {
    apiKeyStatus.textContent = 'Paste your Sarvam API key first';
    return;
  }

  saveApiKeyButton.disabled = true;
  apiKeyStatus.textContent = 'Saving securely...';
  try {
    const message = await invoke('set_api_key', { apiKey });
    apiKeyInput.value = '';
    apiKeyStatus.textContent = message;
  } catch (e) {
    apiKeyStatus.textContent = `Could not save key: ${e}`;
  } finally {
    saveApiKeyButton.disabled = false;
  }
});

removeApiKeyButton.addEventListener('click', async () => {
  removeApiKeyButton.disabled = true;
  try {
    const message = await invoke('clear_api_key');
    apiKeyInput.value = '';
    apiKeyStatus.textContent = message;
  } catch (e) {
    apiKeyStatus.textContent = `Could not remove key: ${e}`;
  } finally {
    removeApiKeyButton.disabled = false;
  }
});

void listen('dictation-started', async () => {
  startDictationUi();
  if (evaluationModeCheck.checked) {
    evaluationStatus.textContent = 'Recording one utterance for the V3/V4 comparison...';
  }
});

void listen('dictation-status', async (event) => {
  showMicStatus(event.payload);
  if (evaluationModeCheck.checked) {
    evaluationStatus.textContent = event.payload;
  }
});

void listen('dictation-result', async (event) => {
  renderDictationResult(event.payload);
});

void listen('dictation-finished', async () => {
  stopDictationUi();
  statusDot.className = 'status-dot ready';
  if (evaluationModeCheck.checked) {
    if (!evaluationRunReceived) {
      await refreshEvaluationResults();
    }
    showMicStatus('Evaluation complete; no transcript was pasted');
  } else {
    showMicStatus('Dictation complete and inserted into the target application');
  }
});

void listen('evaluation-result', async (event) => {
  renderEvaluationRun(event.payload);
  const session = await invoke('get_evaluation_session');
  renderEvaluationHistory(session.runs || []);
});

void listen('dictation-error', async (event) => {
  stopDictationUi();
  statusDot.className = 'status-dot ready';
  statusText.textContent = `Error: ${event.payload}`;
  showMicStatus('Dictation failed');
  if (evaluationModeCheck.checked) {
    evaluationStatus.textContent = `Evaluation failed: ${event.payload}`;
  }
});

void listen('hotkey-status', async (event) => {
  hotkeyStatus.textContent = event.payload;
});

testMicButton.addEventListener('click', async () => {
  if (isDictating) return;
  statusDot.className = 'status-dot recording';
  statusText.textContent = 'Testing mic...';
  showMicStatus('Requesting microphone permission...');
  recordingIndicator.textContent = 'Testing microphone...';
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
      showMicStatus(`Mic open: ${stream.getAudioTracks()[0]?.label || 'default input'} · level ${level}%`);

      if (Date.now() - startedAt < 5000) {
        requestAnimationFrame(tick);
      } else {
        stream.getTracks().forEach((track) => track.stop());
        audioContext.close();
        statusDot.className = 'status-dot ready';
        statusText.textContent = 'Mic test complete';
        showMicStatus('Mic stream closed');
        recordingIndicator.textContent = 'Not recording';
      }
    };

    tick();
  } catch (e) {
    statusDot.className = 'status-dot ready';
    statusText.textContent = `Error: ${e}`;
    showMicStatus('Mic test failed');
    recordingIndicator.textContent = 'Not recording';
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
  rawTranscriptBox.value = '';
  finalTranscriptBox.value = '';
  diagnosticsBox.innerHTML = '<span>Waiting for transcription...</span>';

  try {
    await invoke('toggle_dictation');
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
    const result = await invoke('reset_recording');
    showMicStatus(result === 'no-active-recording'
      ? 'No active recording'
      : 'Stop requested; waiting for recording shutdown');
  } catch (e) {
    statusDot.className = 'status-dot ready';
    statusText.textContent = `Error: ${e}`;
    showMicStatus('Reset failed');
  }
});

copyTranscriptButton.addEventListener('click', copyTranscript);

evaluationModeCheck.addEventListener('change', async () => {
  applyEvaluationMode(evaluationModeCheck.checked);
  await saveEvaluationSettings();
});

expectedTextInput.addEventListener('input', scheduleEvaluationSettingsSave);

refreshEvaluationButton.addEventListener('click', refreshEvaluationResults);

exportEvaluationButton.addEventListener('click', async () => {
  try {
    evaluationStatus.textContent = await invoke('export_evaluation_results');
  } catch (e) {
    evaluationStatus.textContent = `Export failed: ${e}`;
  }
});

clearEvaluationButton.addEventListener('click', async () => {
  try {
    const session = await invoke('clear_evaluation_session');
    expectedTextInput.value = '';
    evaluationResults.hidden = true;
    evaluationV3Transcript.value = '';
    evaluationV4Transcript.value = '';
    renderEvaluationHistory(session.runs || []);
    evaluationStatus.textContent = 'Evaluation session cleared from memory.';
  } catch (e) {
    evaluationStatus.textContent = `Clear failed: ${e}`;
  }
});

loadConfig();

setInterval(() => {
  void refreshEvaluationResults();
}, 750);
