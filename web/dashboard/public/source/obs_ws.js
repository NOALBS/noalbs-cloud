class ReconnectingWebSocket {
	constructor({ url, onOpen, onMessage, onStateChange, onClose, onError, onConnectedPayload, reconnectDelay = 500, maxReconnectDelay = 10000 }) {
		this.url = url;
		this.onOpen = onOpen;
		this.onMessage = onMessage;
		this.onStateChange = onStateChange;
		this.onClose = onClose;
		this.onError = onError;
		this.onConnectedPayload = onConnectedPayload;
		this.socket = null;
		this.shouldReconnect = false;
		this.reconnectTimer = null;
		this.reconnectDelay = reconnectDelay;
		this.initialReconnectDelay = reconnectDelay;
		this.maxReconnectDelay = maxReconnectDelay;
		this.attempt = 0;
	}

	get isOpen() {
		return this.socket && this.socket.readyState === WebSocket.OPEN;
	}

	start() {
		this.shouldReconnect = true;
		this.connect();
	}

	connect() {
		if (!this.url) {
			this.updateState('disconnected');
			return;
		}

		if (this.socket && (this.socket.readyState === WebSocket.CONNECTING || this.socket.readyState === WebSocket.OPEN)) {
			return;
		}

		this.clearReconnectTimer();
		this.updateState(this.attempt > 0 ? 'reconnecting' : 'connecting');
		const attemptId = ++this.attempt;
		const socket = new WebSocket(this.url);
		this.socket = socket;

		socket.onopen = () => {
			if (this.socket !== socket || attemptId !== this.attempt) return;
			this.reconnectDelay = this.initialReconnectDelay;
			this.updateState('connected');
			this.onOpen?.(socket);
			const payload = this.onConnectedPayload?.();
			if (payload !== undefined && payload !== null) {
				this.send(payload);
			}
		};

		socket.onmessage = (event) => {
			if (this.socket !== socket || attemptId !== this.attempt) return;
			this.onMessage?.(event, socket);
		};

		socket.onclose = (event) => {
			if (this.socket === socket) {
				this.socket = null;
			}
			this.onClose?.(event, socket);
			if (this.shouldReconnect) {
				this.scheduleReconnect();
			} else {
				this.updateState('disconnected');
			}
		};

		socket.onerror = (event) => {
			if (this.socket !== socket || attemptId !== this.attempt) return;
			this.onError?.(event, socket);
		};
	}

	scheduleReconnect() {
		if (!this.shouldReconnect) return;
		this.clearReconnectTimer();
		const delay = this.reconnectDelay;
		this.updateState('reconnecting', delay);
		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = null;
			this.connect();
		}, delay);
		this.reconnectDelay = Math.min(this.reconnectDelay * 2, this.maxReconnectDelay);
	}

	stop() {
		this.shouldReconnect = false;
		this.clearReconnectTimer();
		this.closeSocket();
		this.updateState('disconnected');
	}

	closeSocket() {
		const socket = this.socket;
		this.socket = null;
		if (socket) {
			try {
				socket.close();
			} catch {}
		}
	}

	clearReconnectTimer() {
		if (this.reconnectTimer !== null) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
	}

	updateState(state, detail) {
		this.onStateChange?.(state, detail);
	}

	send(payload) {
		if (!this.isOpen) return false;
		this.socket.send(typeof payload === 'string' ? payload : JSON.stringify(payload));
		return true;
	}
}

// How often audio levels go to obs-link at most (it sends 5 a second on).
const METERS_INTERVAL_MS = 200;

const DROPPED_EVENTS = new Set(['InputSettingsChanged']);

class ObsWsRelayAppCore {
	constructor(upstreamDomain = 'localhost:11000', options = {}) {
		this.upstreamDomain = upstreamDomain;
		this.options = {
			upstreamToken: options.upstreamToken ?? '',
			upstreamVersion: options.upstreamVersion ?? '0.1.0',
			logger: options.logger ?? console,
			obsHost: options.obsHost ?? '127.0.0.1',
			obsPort: options.obsPort ?? null,
			obsPassword: options.obsPassword ?? '',
		};
		this.obsReady = false;
		this.upReady = false;
		this.upstreamIdentifySent = false;
		this.identifyAckFailed = false;
		this.obsPort = this.options.obsPort;
		this.obsPassword = this.options.obsPassword;
		this.obsConnection = null;
		this.upConnection = null;
		this.obsRequestTokens = new Map();
		this.pendingMeters = new Map();
		this.metersSentAt = 0;
	}

	get upstreamUrl() {
		const domain = String(this.upstreamDomain || '').trim();
		if (!domain) return '';
		if (/^wss?:\/\//i.test(domain)) return domain;
		return `ws://${domain}`;
	}

	get obsUrl() {
		if (!this.obsPort) return '';
		return `ws://${this.options.obsHost}:${this.obsPort}`;
	}

	get hasObsConfig() {
		return Boolean(this.obsPort);
	}

	log(prefix, value) {
		const logger = this.options.logger;
		const rendered = typeof value === 'string' ? value : this.stringify(value);
		if (logger && typeof logger.log === 'function') {
			logger.log(`${prefix} ${rendered}`);
		}
	}

	stringify(value) {
		try {
			return JSON.stringify(value);
		} catch {
			return String(value);
		}
	}

	async sha256Base64(text) {
		const data = new TextEncoder().encode(text);
		const digest = await crypto.subtle.digest('SHA-256', data);
		let binary = '';
		for (const byte of new Uint8Array(digest)) binary += String.fromCharCode(byte);
		return btoa(binary);
	}

	async makeObsAuth(hello) {
		const auth = hello?.d?.authentication;
		const password = this.obsPassword;
		if (!auth || !password) return null;
		const secret = await this.sha256Base64(password + auth.salt);
		return this.sha256Base64(secret + auth.challenge);
	}

	buildIdentifyPayload() {
		const token = this.options.upstreamToken;
		return {
			token,
			identify: {
				token,
				version: this.options.upstreamVersion,
			},
		};
	}

	parseMaybeJson(value) {
		if (typeof value !== 'string') return value;
		try {
			return JSON.parse(value);
		} catch {
			return value;
		}
	}

	extractObsRawRequest(message) {
		if (!message || typeof message !== 'object') return null;
		const rawRequest = message.obsRawRequest ?? message.obs_raw_request;
		if (!rawRequest) return null;
		return this.parseMaybeJson(rawRequest);
	}

	getRequestId(message) {
		if (!message || typeof message !== 'object') return null;
		return message?.d?.requestId ?? message?.d?.request_id ?? message?.requestId ?? null;
	}

	extractRequestId(payload) {
		if (!payload || typeof payload !== 'object') return null;
		return payload?.d?.requestId ?? payload?.d?.request_id ?? payload?.requestId ?? null;
	}

	registerObsRequestToken(requestId, token) {
		if (!requestId || !token) return;
		this.obsRequestTokens.set(String(requestId), String(token));
	}

	forwardObsRawRequest(message) {
		const request = this.extractObsRawRequest(message);
		if (!request) return false;

		const token = message.token;
		const requestId = this.extractRequestId(request);
		if (requestId && token) {
			this.registerObsRequestToken(requestId, token);
		}

		if (!this.obsConnection || !this.obsReady) {
			return false;
		}

		return this.sendToObs(request);
	}

	forwardObsRawResponse(message) {
		if (message?.op !== 7) return false;
		const requestId = this.getRequestId(message) ?? this.extractRequestId(message);
		if (!requestId) return false;

		const token = this.obsRequestTokens.get(String(requestId));
		if (!token) return false;

		this.obsRequestTokens.delete(String(requestId));
		return this.sendToUpstream({
			token,
			obsRawResponse: message,
		});
	}

	forwardObsRawEvent(message) {
		return this.sendToUpstream({
			obsRawEvent: message,
		});
	}

	/**
	 * OBS sends audio levels 20 times a second (only while obs-link asks for
	 * them); obs-link passes on 5, so this sends no more, from the streamer's
	 * upload: the loudest of each input's channels since the last one, silent
	 * inputs left out, numbers cut to 3 significant digits.
	 */
	forwardVolumeMeters(message) {
		for (const input of message.d?.eventData?.inputs ?? []) {
			const levels = input.inputLevelsMul ?? [];
			const pending = this.pendingMeters.get(input.inputName);
			if (!pending) {
				this.pendingMeters.set(input.inputName, levels.map((channel) => [...channel]));
				continue;
			}
			levels.forEach((channel, i) => {
				if (!pending[i]) pending[i] = [...channel];
				else channel.forEach((value, j) => { pending[i][j] = Math.max(pending[i][j] ?? 0, value); });
			});
		}

		const now = Date.now();
		if (now - this.metersSentAt < METERS_INTERVAL_MS) return false;
		this.metersSentAt = now;
		const inputs = [];
		for (const [inputName, levels] of this.pendingMeters) {
			if (levels.some((channel) => channel.some((value) => value > 0))) {
				inputs.push({
					inputName,
					inputLevelsMul: levels.map((channel) => channel.map((value) => Number(value.toPrecision(3)))),
				});
			}
		}
		this.pendingMeters.clear();
		return this.forwardObsRawEvent({ op: 5, d: { eventType: 'InputVolumeMeters', eventData: { inputs } } });
	}

	disableReconnect() {
		this.identifyAckFailed = true;
		if (this.upConnection && this.upConnection instanceof ReconnectingWebSocket) {
			this.upConnection.shouldReconnect = false;
		}
		if (this.obsConnection && this.obsConnection instanceof ReconnectingWebSocket) {
			this.obsConnection.shouldReconnect = false;
		}
	}

	extractObsConfig(message) {
		if (!message || typeof message !== 'object') return null;

		const source = message.settings?.obs
			?? message.d?.settings?.obs
			?? message.d?.obs
			?? message.d
			?? message;
		const port = source.port ?? source.obsPort ?? source.wsPort;
		const password = source.password ?? source.obsPassword ?? source.obs_password ?? source.wsPassword;
		if (port === undefined || port === null) return null;

		return {
			port: Number(port),
			password: password === undefined || password === null ? '' : String(password),
		};
	}

	applyObsConfig(config) {
		if (!config || !Number.isFinite(config.port)) return false;
		this.obsPort = config.port;
		if (config.password !== undefined) {
			this.obsPassword = String(config.password || '');
		}
		this.log('[up]', `obs config received on port ${this.obsPort}`);
		this.restartObsConnection();
		return true;
	}

	restartObsConnection() {
		if (this.obsConnection) {
			this.obsConnection.stop();
			this.obsConnection = null;
		}
		if (!this.hasObsConfig) {
			return;
		}
		this.obsReady = false;
		this.obsConnection = this.createObsConnection();
		this.obsConnection.start();
	}


	createUpstreamConnection() {
		return new ReconnectingWebSocket({
			url: this.upstreamUrl,
			onStateChange: (state) => this.log('[up state]', state),
			onOpen: () => {
				this.upReady = true;
				this.log('[up]', 'connected');
				this.upstreamIdentifySent = true;
				this.sendToUpstream(this.buildIdentifyPayload());
			},
			onMessage: (event) => {
				this.log('[up <-]', event.data);
				const msg = this.parseMaybeJson(event.data);

				if (msg && typeof msg === 'object' && (Object.prototype.hasOwnProperty.call(msg, 'obsRawRequest') || Object.prototype.hasOwnProperty.call(msg, 'obs_raw_request'))) {
					this.forwardObsRawRequest(msg);
					return;
				}

				if (msg && typeof msg === 'object' && (Object.prototype.hasOwnProperty.call(msg, 'identifyAck') || Object.prototype.hasOwnProperty.call(msg, 'identify_ack'))) {
					if (msg.identifyAck === false || msg.identify_ack === false) {
						this.log('[up]', 'identify acknowledged as false; disabling reconnects');
						this.disableReconnect();
					}
					return;
				}

				const config = this.extractObsConfig(msg);
				if (config) {
					this.applyObsConfig(config);
					return;
				}
			},
			onClose: () => {
				this.upReady = false;
				this.upstreamIdentifySent = false;
				this.obsRequestTokens.clear();
				this.obsReady = false;
				if (this.obsConnection) {
					this.obsConnection.stop();
					this.obsConnection = null;
				}
				this.log('[up]', 'closed');
			},
			onError: () => {
				this.log('[up]', 'error');
			},
		});
	}

	createObsConnection() {
		return new ReconnectingWebSocket({
			url: this.obsUrl,
			onStateChange: (state) => this.log('[obs state]', state),
			onOpen: () => {
				this.obsReady = false;
				this.log('[obs]', 'socket open');
			},
			onMessage: async (event) => {
				const msg = this.parseMaybeJson(event.data);
				const eventType = msg?.op === 5 ? msg.d?.eventType : undefined;
				if (DROPPED_EVENTS.has(eventType)) return;
				if (eventType !== 'InputVolumeMeters') this.log('[obs <-]', event.data);
				if (!msg || typeof msg !== 'object') {
					return;
				}

				if (msg.op === 7 && this.forwardObsRawResponse(msg)) {
					return;
				}

				if (msg.op === 0) {
					const identify = { op: 1, d: { rpcVersion: 1 } };
					const auth = await this.makeObsAuth(msg);
					if (auth) identify.d.authentication = auth;
					this.sendToObs(identify);
					return;
				}

				if (msg.op === 2) {
					// Also the answer to a Reidentify (obs-link turning audio levels on or off).
					if (this.obsReady) return;
					this.obsReady = true;
					this.log('[obs]', 'identified');
					this.sendToUpstream({ obsWsConnected: true });
					return;
				}

				if (msg.op === 5) {
					if (eventType === 'InputVolumeMeters') {
						this.forwardVolumeMeters(msg);
						return;
					}
					this.forwardObsRawEvent(msg);
				}
			},
			onClose: () => {
				this.obsReady = false;
				this.sendToUpstream({ obsWsConnected: false });
				this.log('[obs]', 'closed');
			},
			onError: () => {
				this.log('[obs]', 'error');
			},
		});
	}

	start() {
		this.stop();
		this.upConnection = this.createUpstreamConnection();
		this.upConnection.start();
		return this;
	}

	stop() {
		if (this.obsConnection) {
			this.obsConnection.stop();
			this.obsConnection = null;
		}
		if (this.upConnection) {
			this.upConnection.stop();
			this.upConnection = null;
		}
		this.obsReady = false;
		this.upReady = false;
		this.upstreamIdentifySent = false;
	}

	sendToObs(payload) {
		if (!this.obsConnection) {
			return false;
		}
		return this.obsConnection ? this.obsConnection.send(payload) : false;
	}

	sendToUpstream(payload) {
		return this.upConnection ? this.upConnection.send(payload) : false;
	}
}

function ObsWsRelayApp(upstreamDomain = 'localhost:11000', options = {}) {
	return new ObsWsRelayAppCore(upstreamDomain, options);
}

export { ObsWsRelayApp, ObsWsRelayAppCore, ReconnectingWebSocket };

if (typeof window !== 'undefined') {
	window.ObsWsRelayApp = ObsWsRelayApp;
	window.ObsWsRelayAppCore = ObsWsRelayAppCore;
	window.ReconnectingWebSocket = ReconnectingWebSocket;
}

if (typeof module !== 'undefined' && module.exports) {
	module.exports = { ObsWsRelayApp, ObsWsRelayAppCore, ReconnectingWebSocket };
}