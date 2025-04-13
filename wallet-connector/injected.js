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
        // Use the same pendingRequests map as backgroundFetch
        pendingRequests.set(requestId, { resolve, reject });
        console.log(`Injected Script: Sending connection request ${requestId} for origin ${origin}`);
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
                // Reject with a specific error or a generic one
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
        //  console.log("Zeus Provider announced");

        // Listen for requests specifically for this provider
        window.addEventListener("eip6963:requestProvider", (event) => {
            // EIP-6963 specifies that the provider should respond by announcing itself again
            // if it hasn't already been announced or if the dapp specifically requests.
            // Re-announcing is safe.
            this._announceProvider();
        });
    }


    async _initializeState() {
        try {
            const statusData = await backgroundFetch('/status', {
                method: "GET",
                headers: { "Content-Type": "application/json" },
            });

            // Just log the status and continue
            if (statusData.status) {
                console.log("Zeus is running, all good.");
                this._isConnected = true;
                this._accounts = [];
                this._chainId = null;
            } else {
                console.log("Zeus is not running");
                this._isConnected = false;
                this._accounts = [];
                this._chainId = null;
            }

        } catch (e) {
            // Error handling might change slightly depending on what background sends back
            console.error("Error initializing ZeusProvider state:", e);
            // Assume connection failed if init fails
            this._isConnected = false;
            this._accounts = [];
            this._chainId = null;
            // Decide if you want to warn the user more explicitly here
        } finally {
            window.dispatchEvent(new Event("ethereum#initialized"));
            console.log("Dispatched ethereum#initialized event.");
        }
    }


    // --- EIP-1193 Methods ---

    isConnected() {
        // console.log("ZeusProvider isConnected check:", this._isConnected);
        return this._isConnected;
    }



    async request({ method, params }) {
       console.log(`ZeusProvider request received: Method=${method}, Params=`, params);

        try {
            let data;

            if (method === 'eth_requestAccounts' || method === 'wallet_requestPermissions') {
                console.log("Wallet connection requested, sending request to Zeus");
                const requestId = requestIdCounter++; // Generate unique ID for this request
                const origin = window.location.origin;

                try {
                    // Initiate confirmation flow and wait for result
                    const confirmationResult = await backgroundConfirmConnection(origin, requestId);

                    // Background script should resolve with { approved: true, accounts: [...] } on success
                    if (confirmationResult && confirmationResult.approved) {
                        console.log("Connection approved by user. Accounts:", confirmationResult.accounts);
                        // We directly receive the accounts upon approval
                        // Construct the expected JSON-RPC like structure for processing below
                        data = { result: confirmationResult.accounts, error: null };
                    } else {
                        // Should have been rejected by backgroundConfirmConnection, but double-check
                        console.warn("Connection confirmation flow did not return approved status.");
                        throw new Error("User rejected the connection request."); // Default rejection
                    }
                } catch (e) {
                    // Handle rejection or timeout from backgroundConfirmConnection
                    console.error("Connection confirmation failed:", e.message);
                    // Throw EIP-1193 specific error for user rejection
                    const error = new Error(e.message || "User rejected the connection request.");
                    error.code = 4001; // EIP-1193 User Rejected Request
                    throw error;
                }

            } else {
                // For all OTHER methods, use the standard background fetch
                data = await backgroundFetch('/api', {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ jsonrpc: "2.0", id: "bg-" + Date.now(), method, params }),
                });
            }

            // Process the data (whether from confirmation or standard fetch)
            console.log("ZeusProvider processing response data:", data);

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
            // Optional: Trigger disconnect if appropriate
            // if (e.message.includes('Background fetch failed') || e.message.includes('timed out')) {
            //     if (this._isConnected) this._handleDisconnect("Connection to Zeus lost.");
            // }
            // Rethrow a user-friendly error or the specific error
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


}

// --- Injection Check ---
// Prevent multiple injections if script somehow runs twice
if (!window.ethereum?.isZeus) {
    // Instantiate and inject the provider into the window object
    window.ethereum = new ZeusProvider();

    // Optional: You might want to dispatch an event SOME dapps listen for
    // This is not standard EIP-1193 but used by some older patterns
    // Handled by _initializeState now.
    // window.dispatchEvent(new Event('ethereum#initialized'));
    // console.log("Dispatched ethereum#initialized event.");

} else {
    console.warn("Zeus EIP-1193 Provider already injected. Skipping.");
    // If another provider exists, you might want to log it or decide how to handle it.
    // EIP-6963 helps manage multiple providers gracefully.
}