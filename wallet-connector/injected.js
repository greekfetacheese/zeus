class EventEmitter {
    constructor() {
        this.listeners = {};
    }

    on(event, callback) {
        if (!this.listeners[event]) {
            this.listeners[event] = [];
        }
        this.listeners[event].push(callback);
        console.log(`Listener added for event: ${event}`);
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
        if (JSON.stringify(window.ethereum?._accounts || []) !== JSON.stringify(newAccounts)) {
            if (window.ethereum) {
                window.ethereum._accounts = newAccounts;
                // Check connection status based on accounts
                const wasConnected = window.ethereum._isConnected;
                window.ethereum._isConnected = newAccounts.length > 0;

                window.ethereum.emit('accountsChanged', newAccounts); // Emit EIP-1193 event
                console.log("Emitted 'accountsChanged' event.");

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
        if (window.ethereum?._chainId !== newChainId) {
            if (window.ethereum) {
                window.ethereum._chainId = newChainId; // Update internal state
                window.ethereum.emit('chainChanged', newChainId); // Emit EIP-1193 event
                console.log("Emitted 'chainChanged' event.");
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
        try {
            const statusData = await backgroundFetch('/status', { method: "GET", headers: { "Content-Type": "application/json" } });
            if (statusData.status) {
                this._isConnected = true;
                this._accounts = statusData.accounts || [];
                this._chainId = statusData.chainId || null;
                if (this._accounts.length > 0) {
                    this.emit("connect", { chainId: this._chainId });
                }
            }
        } catch (e) {
            this._isConnected = false;
            this.emit("disconnect", new Error("Zeus server unavailable"));
        } finally {
            window.dispatchEvent(new Event("ethereum#initialized"));
        }
    }


    isConnected() {
        return this._isConnected;
    }


    async request({ method, params }) {
        // console.log(`ZeusProvider request received: Method=${method}, Params=`, params);

        try {
            let data;
            const origin = window.location.origin;

            if (method === 'eth_requestAccounts' || method === 'wallet_requestPermissions') {
                console.log("Wallet connection requested, sending request to Zeus");
                const requestId = requestIdCounter++; // Generate unique ID for this request

                try {
                    const confirmationResult = await backgroundConfirmConnection(origin, requestId);

                    if (confirmationResult && confirmationResult.approved) {
                        console.log("Connection approved by user. Accounts:", confirmationResult.accounts);
                        data = { result: confirmationResult.accounts, error: null };
                    } else {
                        console.warn("Connection confirmation flow did not return approved status.");
                        throw new Error("User rejected the connection request.");
                    }
                } catch (e) {
                    console.error("Connection confirmation failed:", e.message);
                    const error = new Error(e.message || "User rejected the connection request.");
                    error.code = 4001;
                    throw error;
                }

            } else {
                // For all OTHER methods, use the standard background fetch
                data = await backgroundFetch('/api', {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ origin: origin, jsonrpc: "2.0", id: "bg-" + Date.now(), method, params }),
                });
            }

            // console.log("ZeusProvider processing response data:", data);

            if (data.error) {
                console.error("Zeus API or Confirmation returned error:", data.error);
                const error = new Error(data.error.message || "Zeus wallet error");
                error.code = data.error.code || -32603;
                error.data = data.error.data;
                throw error;
            }

            const result = data.result;

            return result;

        } catch (e) {
            console.error(`ZeusProvider Error during request ${method}:`, e);
            if (e.message.includes('Background fetch failed') || e.message.includes('timed out')) {
                throw new Error("Connection to Zeus Wallet failed. Please ensure Zeus is running and accessible.");
            }
            throw e;
        }
    }

    // --- Event Handling ---

    // Placeholder for disconnect logic
    _handleDisconnect(reason) {
        console.warn(`ZeusProvider disconnected: ${reason}`);
        const wasConnected = this._isConnected;
        this._isConnected = false;
        this._accounts = [];
        // Don't reset chainId necessarily, it might still be known

        if (wasConnected) {
            // EIP-1193 specifies a 'disconnect' event with an Error object
            const error = new Error(reason);
            error.code = 1013; // Example code for provider disconnect
            this.emit("disconnect", error);
            console.log("Emitted 'disconnect' event.");
            // Also emit accountsChanged with empty array
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