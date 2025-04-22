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
// Simple way to handle pending requests and correlate responses
const pendingRequests = new Map();
let requestIdCounter = 0;

// Listen for responses relayed from the content script
window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data || event.data.target !== 'injected') {
        return;
    }
    const message = event.data;
    // console.log('Injected Script: Received message from content:', message);

    if (message.type === 'fetch_response' && pendingRequests.has(message.id)) {
        const { resolve, reject } = pendingRequests.get(message.id);
        pendingRequests.delete(message.id); // Clean up

        if (message.success) {
            resolve(message.data);
        } else {
            console.error('Injected Script: Received error response:', message.error);
            reject(new Error(message.error || 'Background fetch failed'));
        }
    } // ***** Handle State Change Events from Background *****
    else if (message.type === 'accountsChanged') {
        // console.log('Injected Script: Received accountsChanged event:', message.payload);
        const newAccounts = message.payload || [];
        const currentAccountsJson = JSON.stringify(window.ethereum?._accounts || []);
        const newAccountsJson = JSON.stringify(newAccounts);

        console.log('Comparing accounts:', currentAccountsJson, 'vs', newAccountsJson);


        // Compare with current state to avoid redundant emits
        if (currentAccountsJson !== newAccountsJson) {
            if (window.ethereum) {
                window.ethereum._accounts = newAccounts;
                const wasConnected = window.ethereum._isConnected;
                window.ethereum._isConnected = newAccounts.length > 0;
                window.ethereum.emit('accountsChanged', newAccounts); // Emit from poll
                console.log("Emitted 'accountsChanged' event (from poll).");

                // Handle disconnect transition
                if (wasConnected && !window.ethereum._isConnected) {
                    console.log("Injected Script: Accounts empty, emitting disconnect.");
                    // Construct EIP-1193 disconnect error
                    const error = new Error("Provider disconnected.");
                    error.code = 4900;
                    window.ethereum.emit('disconnect', error);
                }
            }
        }
    } else if (message.type === 'chainChanged') {
        // console.log('Injected Script: Received chainChanged event:', message.payload);
        const newChainId = message.payload || null;
        const currentChainId = window.ethereum?._chainId;

        console.log('Comparing chainId:', currentChainId, 'vs', newChainId);

        // Compare with current state
        if (currentChainId !== newChainId) {
            if (window.ethereum) {
                window.ethereum._chainId = newChainId; // Update from poll
                window.ethereum.emit('chainChanged', newChainId); // Emit from poll
                console.log("Emitted 'chainChanged' event (from poll).");
            }
        }
    }
});



// Function to replace fetch
function backgroundFetch(url, options) {
    return new Promise((resolve, reject) => {
        const requestId = requestIdCounter++;
        pendingRequests.set(requestId, { resolve, reject });
        // console.log(`Injected Script: Sending fetch request ${requestId} for ${url}`);
        window.postMessage(
            {
                target: 'content',     // Send to content script
                type: 'fetch_request', // Indicate it's a fetch request
                id: requestId,         // Unique ID for correlation
                payload: { url, options } // The original fetch parameters
            },
            "*" // Target origin (use "*" for simplicity here, or specific origin if needed)
        );

        // Optional: Add a timeout for requests
        setTimeout(() => {
            if (pendingRequests.has(requestId)) {
                pendingRequests.delete(requestId);
                reject(new Error(`Request ${requestId} timed out after 30 seconds`));
            }
        }, 30000); // 30 second timeout
    });
}

function backgroundConfirmConnection(origin, requestId) {
    return new Promise((resolve, reject) => {
        pendingRequests.set(requestId, { resolve, reject });
        window.postMessage(
            {
                target: 'content',
                type: 'connection_request',
                id: requestId,
                payload: { origin } // Send dapp origin
            },
            "*"
        );

        // Timeout for user action (e.g., 5 minutes)
        setTimeout(() => {
            if (pendingRequests.has(requestId)) {
                pendingRequests.delete(requestId);
                console.warn(`Connection request ${requestId} timed out waiting for user action.`);
                reject(new Error('Connection request timed out.'));
            }
        }, 300000); // 5 minutes timeout
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

    // EIP-6963: Announce the provider presence
    _announceProvider() {
        const announceEvent = new CustomEvent("eip6963:announceProvider", {
            detail: Object.freeze({
                info: {
                    uuid: crypto.randomUUID(),
                    name: "Zeus Wallet",
                    icon: "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'%3E%3Ctext x='50' y='50' font-size='50' text-anchor='middle' dy='.3em'%3E⚡%3C/text%3E%3C/svg%3E", // Basic lightning bolt emoji icon
                    rdns: "io.github.zeus-wallet" // TODO
                },
                provider: this
            })
        });
        window.dispatchEvent(announceEvent);
        // Listen for requests specifically for this provider
        window.addEventListener("eip6963:requestProvider", (event) => {
            this._announceProvider();
        });
    }


    async _initializeState() {
        console.log("ZeusProvider initializing...");
        try {
            await backgroundFetch('/status', {
                method: "GET",
                headers: { "Content-Type": "application/json" },
            });
            console.log("Zeus server is running.");
        } catch (e) {
            console.error("Error initializing ZeusProvider state (server potentially down):", e);
            this._isConnected = false;
        } finally {
            window.dispatchEvent(new Event("ethereum#initialized"));
            console.log("Dispatched ethereum#initialized event.");
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

            // --- Update State and Emit Events ---
            if (method === 'eth_requestAccounts' || method === 'wallet_getPermissions') {
                const newAccounts = result || [];
                const changed = JSON.stringify(this._accounts || []) !== JSON.stringify(newAccounts);
                const wasConnected = this._isConnected;

                this._accounts = newAccounts;
                this._isConnected = true;   

                 // Emit events AFTER state is updated
                 if (!wasConnected) {
                     if (!this._chainId) {
                          try {
                              const chainData = await backgroundFetch('/api', {
                                   method: "POST", headers: { "Content-Type": "application/json" },
                                   body: JSON.stringify({ origin: origin, jsonrpc: "2.0", id: "bg_chain", method: 'eth_chainId', params: [] })
                              });
                              if (!chainData.error) this._chainId = chainData.result;
                          } catch(e) { console.error("Failed to get chainId for connect event:", e); }
                     }
                      this.emit("connect", { chainId: this._chainId });
                 }
                 if (changed) {
                     this.emit("accountsChanged", this._accounts);
                 }
            } else if (method === 'eth_chainId') {
                 const newChainId = result;
                 if (this._chainId !== newChainId) {
                     console.log(`Chain ID updated via request: ${newChainId}`);
                     this._chainId = newChainId;
                     this.emit("chainChanged", this._chainId);
                 }
            }
            else if (method === 'wallet_switchEthereumChain' && result === null /* EIP-1193 success is null */) {
                 console.log("wallet_switchEthereumChain successful, polling will update chainId state.");
            }
            
            return result;

        } catch (e) {
            console.error(`ZeusProvider Error during request ${method}:`, e);
             if (e.code === 4001 || e.code === 4900 || e.code === 4902 || e.code === -32601 || e.code === -32602 || e.code === -32603) {
                 throw e;
             }
             if (e.message.includes('Background fetch failed') || e.message.includes('timed out')) {
                 this._isConnected = false;
                 if (this._accounts?.length > 0) this.emit('accountsChanged', []);
                 this.emit('disconnect', new Error("Connection to Zeus Wallet failed."));
                 throw new Error("Connection to Zeus Wallet failed. Please ensure Zeus is running and accessible.");
             }
             throw e;
        }
    }

    // --- Event Handling ---

    _handleDisconnect(reason) {
        console.warn(`ZeusProvider disconnected: ${reason}`);
        const wasConnected = this._isConnected;
        this._isConnected = false;
        this._accounts = [];

        if (wasConnected) {
            const error = new Error(reason);
            error.code = 1013;
            this.emit("disconnect", error);
            console.log("Emitted 'disconnect' event.");
            this.emit("accountsChanged", []);
            console.log("Emitted 'accountsChanged' event (empty).");
        }
    }

    // Legacy Support
    async enable() {
        return this.request({ method: "eth_requestAccounts" });
    }
    async send(method, params) {
        return this.request({ method, params });
    }
}

// --- Injection Check ---
// Prevent multiple injections if script somehow runs twice
if (!window.ethereum?.isZeus) {
    window.ethereum = new ZeusProvider();
} else {
    console.warn("Zeus EIP-1193 Provider already injected. Skipping.");
}