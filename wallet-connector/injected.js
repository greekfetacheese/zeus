const MAX_LISTENERS = 100;

class EventEmitter {
    constructor() {
        this.listeners = {};
        this.maxListeners = MAX_LISTENERS;
    }

    on(event, callback) {
        if (!this.listeners[event]) {
            this.listeners[event] = [];
        }
        if (this.listeners[event].length >= this.maxListeners) {
            console.warn(`Zeus: Possible memory leak - ${this.listeners[event].length + 1} listeners added for '${event}'. Use setMaxListeners to increase.`);
        }
        this.listeners[event].push(callback);
        return this;
    }

    addListener(event, callback) {
        return this.on(event, callback);
    }

    once(event, callback) {
        const wrapper = (...args) => {
            this.removeListener(event, wrapper);
            callback(...args);
        };
        return this.on(event, wrapper);
    }

    removeListener(event, callback) {
        if (!this.listeners[event]) return this;
        this.listeners[event] = this.listeners[event].filter(cb => cb !== callback);
        if (this.listeners[event].length === 0) {
            delete this.listeners[event];
        }
        return this;
    }

    off(event, callback) {
        return this.removeListener(event, callback);
    }

    emit(event, ...args) {
        if (this.listeners[event]) {
            this.listeners[event].slice().forEach(callback => {
                try {
                    callback(...args);
                } catch (e) {
                    console.error(`Error in listener for event ${event}:`, e);
                }
            });
        }
        return this;
    }

    setMaxListeners(n) {
        this.maxListeners = n;
        return this;
    }

    removeAllListeners(event) {
        if (event) {
            delete this.listeners[event];
        } else {
            this.listeners = {};
        }
        return this;
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
                // console.log("Zeus: Received chainChanged from background. Updating state:", newChainId);
                window.ethereum._chainId = newChainId;
                window.ethereum.emit('chainChanged', newChainId);
            }
        }
    }
});

const FIVE_MINUTES = 60000 * 5;
const ZEUS_PROVIDER_UUID = crypto.randomUUID();

function accountsFromPermissions(result) {
    if (!Array.isArray(result)) return [];
    const perm = result.find(p => p && p.parentCapability === 'eth_accounts');
    if (!perm || !Array.isArray(perm.caveats)) return [];
    const caveat = perm.caveats.find(c => c && c.type === 'restrictReturnedAccounts');
    return (caveat && Array.isArray(caveat.value)) ? caveat.value : [];
}

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
        this.setMaxListeners(MAX_LISTENERS);
        this._initializeState();
        this._announceProvider();
        window.addEventListener("eip6963:requestProvider", () => {
            this._announceProvider();
        });
    }

    _announceProvider() {
        const announceEvent = new CustomEvent("eip6963:announceProvider", {
            detail: Object.freeze({
                info: {
                    uuid: ZEUS_PROVIDER_UUID,
                    name: "Zeus",
                    // EIP-6963 wants a data URI: the Zeus mark as a PNG. An emoji glyph
                    // inside an SVG renders as a generic bolt in dapp modals.
                    icon: "data:image/png;base64,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",
                    rdns: "io.github.zeus-wallet"
                },
                provider: this
            })
        });
        window.dispatchEvent(announceEvent);
    }

    async _initializeState() {
        console.log("ZeusProvider initializing...");
        try {
            this._chainId = await this.request({ method: 'eth_chainId' });
            this._accounts = await this.request({ method: 'eth_accounts' });
            this._isConnected = this._accounts.length > 0;
            // console.log("Initial state:", { chainId: this._chainId, accounts: this._accounts });
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

    async _applyAccounts(newAccounts) {
        const currentAccountsJson = JSON.stringify(this._accounts || []);
        const newAccountsJson = JSON.stringify(newAccounts);
        const wasConnected = this._isConnected;
        this._accounts = newAccounts;
        this._isConnected = newAccounts.length > 0;

        if (currentAccountsJson !== newAccountsJson) {
            this.emit('accountsChanged', newAccounts);
        }

        if (!wasConnected && this._isConnected) {
            try {
                const chainData = await this.request({ method: 'eth_chainId' });
                this._chainId = chainData;
                this.emit("connect", { chainId: this._chainId });
            } catch (e) {
                this.emit("connect", {});
            }
        }
    }

    async request({ method, params }) {
        // console.log(`Zeus: request received: Method=${method}, Params=`, params);

        try {
            const response = await backgroundFetch('/api', {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                    jsonrpc: "2.0",
                    id: "bg-" + Date.now(),
                    method: method,
                    params: params
                }),
            });

            if (response.error) {
                // console.error("Zeus API returned error:", response.error);
                const error = new Error(response.error.message || "Zeus wallet error");
                error.code = response.error.code || -32603;
                error.data = response.error.data;
                throw error;
            }

            const result = response.result;

            if (method === 'eth_requestAccounts') {
                await this._applyAccounts(Array.isArray(result) ? result : []);
            } else if (method === 'wallet_requestPermissions') {
                const fromPerms = accountsFromPermissions(result);
                if (fromPerms.length > 0) {
                    await this._applyAccounts(fromPerms);
                } else {
                    try {
                        const accounts = await this.request({ method: 'eth_accounts' });
                        await this._applyAccounts(Array.isArray(accounts) ? accounts : []);
                    } catch (e) {
                        // permissions granted but accounts fetch failed; leave state
                    }
                }
            }

            // EIP-1193: dapps (Beefy/web3-onboard, etc.) update UI from chainChanged,
            // not from the wallet_switchEthereumChain result. Emitting only via the
            // background /status poll is too late and often never arrives (MV3 SW).
            if (method === 'wallet_switchEthereumChain' || method === 'wallet_addEthereumChain') {
                const requested = params && params[0] && params[0].chainId;
                const newChainId = requested || await this.request({ method: 'eth_chainId' });
                if (newChainId && this._chainId !== newChainId) {
                    this._chainId = newChainId;
                    this.emit('chainChanged', newChainId);
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
            // console.log("Emitted 'disconnect' event.");
            this.emit("accountsChanged", []);
            // console.log("Emitted 'accountsChanged' event (empty).");
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