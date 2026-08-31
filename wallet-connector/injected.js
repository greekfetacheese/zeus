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
                    name: "Zeus Wallet",
                    icon: "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Ctext x='50' y='50' font-size='50' text-anchor='middle' dy='.3em'%3E⚡%3C/text%3E%3C/svg%3E",
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