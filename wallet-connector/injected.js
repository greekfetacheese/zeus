class EventEmitter {
    constructor() {
        this.listeners = {};
    }

    on(event, callback) {
        if (!this.listeners[event]) {
            this.listeners[event] = [];
        }
        this.listeners[event].push(callback);
        console.log(`Zeus: Listener added for event: ${event}`);
    }

    emit(event, ...args) {
        console.log(`Emitting event: ${event}`, args);
        if (this.listeners[event]) {
            this.listeners[event].forEach(callback => {
                try {
                    callback(...args);
                } catch (e) {
                    console.error(`Error in listener for event ${event}:`, e);
                }
            });
        }
    }

    removeAllListeners(event) {
        if (event) {
            delete this.listeners[event];
        } else {
            this.listeners = {};
        }
    }
}


// --- Request Management ---
const pendingRequests = new Map();
let requestIdCounter = 0;

// --- Injected Script's own listener for messages from content.js ---
window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data || event.data.target !== 'injected') {
        return;
    }
    const message = event.data;

    // Handle responses to fetch/connection requests
    if (message.type === 'fetch_response' && pendingRequests.has(message.id)) {
        const { resolve, reject } = pendingRequests.get(message.id);
        pendingRequests.delete(message.id);

        if (message.success) {
            resolve(message.data);
        } else {
            reject(new Error(message.error || 'Background fetch failed'));
        }
    }
    // Handle state changes pushed from the background script (via content script)
    else if (message.type === 'accountsChanged') {
        const newAccounts = message.payload || [];
        if (window.ethereum && window.ethereum.isZeus) {
            const currentAccountsJson = JSON.stringify(window.ethereum._accounts || []);
            const newAccountsJson = JSON.stringify(newAccounts);

            if (currentAccountsJson !== newAccountsJson) {
                console.log("Zeus: Received accountsChanged from background. Updating state:", newAccounts);
                window.ethereum._accounts = newAccounts;
                const wasConnected = window.ethereum._isConnected;
                window.ethereum._isConnected = newAccounts.length > 0;

                window.ethereum.emit('accountsChanged', newAccounts);

                if (wasConnected && !window.ethereum._isConnected) {
                    const disconnectError = new Error("Provider disconnected.");
                    disconnectError.code = 4900;
                    window.ethereum.emit('disconnect', disconnectError);
                }
            }
        }
    } else if (message.type === 'chainChanged') {
        const newChainId = message.payload || null;
        if (window.ethereum && window.ethereum.isZeus) {
            const currentChainId = window.ethereum._chainId;
            if (currentChainId !== newChainId) {
                console.log("Zeus: Received chainChanged from background. Updating state:", newChainId);
                window.ethereum._chainId = newChainId;
                window.ethereum.emit('chainChanged', newChainId);
            }
        }
    }
});

const FIVE_MINUTES = 60000 * 5;

function backgroundFetch(url, options) {
    return new Promise((resolve, reject) => {
        const requestId = requestIdCounter++;
        pendingRequests.set(requestId, { resolve, reject });
        window.postMessage(
            {
                target: 'content',
                type: 'fetch_request',
                id: requestId,
                payload: { url, options }
            },
            "*"
        );
        setTimeout(() => {
            if (pendingRequests.has(requestId)) {
                pendingRequests.delete(requestId);
                reject(new Error(`Request ${requestId} timed out after 30 seconds`));
            }
        }, FIVE_MINUTES);
    });
}


class ZeusProvider extends EventEmitter {
    constructor() {
        super();
        this.isZeus = true;
        this._isConnected = false;
        this._accounts = [];
        this._chainId = null;
        this._initializeState();
        this._announceProvider();
    }

    _announceProvider() {
        const announceEvent = new CustomEvent("eip6963:announceProvider", {
            detail: Object.freeze({
                info: {
                    uuid: crypto.randomUUID(),
                    name: "Zeus Wallet",
                    icon: "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Ctext x='50' y='50' font-size='50' text-anchor='middle' dy='.3em'%3E⚡%3C/text%3E%3C/svg%3E",
                    rdns: "io.github.zeus-wallet"
                },
                provider: this
            })
        });
        window.dispatchEvent(announceEvent);
        window.addEventListener("eip6963:requestProvider", (event) => {
            this._announceProvider();
        });
    }

    async _initializeState() {
        console.log("ZeusProvider initializing...");
        try {
            this._chainId = await this.request({ method: 'eth_chainId' });
            this._accounts = await this.request({ method: 'eth_accounts' });
            this._isConnected = this._accounts.length > 0;
            console.log("Initial state:", { chainId: this._chainId, accounts: this._accounts });
        } catch (e) {
            console.error("Error initializing:", e);
            this._isConnected = false;
            this._accounts = [];
        } finally {
            window.dispatchEvent(new Event("ethereum#initialized"));
        }
    }

    isConnected() {
        return this._isConnected;
    }

    async request({ method, params }) {
        console.log(`Zeus: request received: Method=${method}, Params=`, params);
        const origin = window.location.origin;

        try {
            const response = await backgroundFetch('/api', {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    origin: origin,
                    jsonrpc: "2.0",
                    id: "bg-" + Date.now(),
                    method: method,
                    params: params
                }),
            });

            if (response.error) {
                console.error("Zeus API returned error:", response.error);
                const error = new Error(response.error.message || "Zeus wallet error");
                error.code = response.error.code || -32603;
                error.data = response.error.data;
                throw error;
            }

            const result = response.result;

            if (method === 'eth_requestAccounts' || method === 'wallet_requestPermissions') {
                const newAccounts = result || [];
                const wasConnected = this._isConnected;
                this._accounts = newAccounts;
                this._isConnected = newAccounts.length > 0;

                if (!wasConnected && this._isConnected) {
                    try {
                        const chainData = await this.request({ method: 'eth_chainId' });
                        this._chainId = chainData;
                        this.emit("connect", { chainId: this._chainId });
                        console.log("Zeus: Emitted 'connect' event.", { chainId: this._chainId });
                    } catch (e) {
                        console.error("Zeus: Failed to get chainId for connect event:", e);
                        this.emit("connect", {});
                    }
                }
            }

            return result;

        } catch (e) {
            console.error(`ZeusProvider Error during request ${method}:`, e);
            if (e.message.includes('Background fetch failed') || e.message.includes('timed out')) {
                this._handleDisconnect("Connection to Zeus Wallet failed.");
            }
            throw e;
        }
    }

    _handleDisconnect(reason) {
        console.warn(`ZeusProvider disconnected: ${reason}`);
        const wasConnected = this._isConnected;
        this._isConnected = false;
        this._accounts = [];

        if (wasConnected) {
            const error = new Error(reason);
            error.code = 4900;
            this.emit("disconnect", error);
            console.log("Emitted 'disconnect' event.");
            this.emit("accountsChanged", []);
            console.log("Emitted 'accountsChanged' event (empty).");
        }
    }

    async enable() {
        return this.request({ method: "eth_requestAccounts" });
    }
    async send(method, params) {
        return this.request({ method, params });
    }
}

// --- Injection Check ---
const provider = new ZeusProvider();
if (!window.ethereum) {
    window.ethereum = provider;
    console.log("Zeus: Set as default window.ethereum provider.");
} else {
    console.warn("Zeus: Existing Ethereum provider detected. Announcing via EIP-6963 only, no overwrite.");
}