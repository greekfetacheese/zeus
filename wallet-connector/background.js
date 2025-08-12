const SERVER_PORT = 65534;
const SERVER_URL_STATUS = `http://127.0.0.1:${SERVER_PORT}/status`;
const SERVER_URL_API = `http://127.0.0.1:${SERVER_PORT}/api`;
const SERVER_URL_REQUEST_CONNECTION = `http://127.0.0.1:${SERVER_PORT}/request-connection`;

const CONNECTION_REQUEST_TIMEOUT_MS = 30000; // 30 seconds
const POLLING_INTERVAL_MS = 1000; // Poll every 1 second

// --- State Variables ---
let lastKnownAccounts = null; // Store as JSON string for easy comparison
let lastKnownChainId = null;
let isFirstPoll = true; // Flag for initial poll
let lastKnownConnectedOrigins = JSON.stringify([]);


async function pollServerStatus() {
    try {
        const response = await fetch(SERVER_URL_STATUS);
        if (!response.ok) return;
        const currentState = await response.json();

        const currentAccounts = currentState.accounts || [];
        const currentChainId = currentState.chainId || null;
        const currentOrigins = (currentState.connectedOrigins || []).slice().sort();
        const originsJson = JSON.stringify(currentOrigins);

        const accountsJson = JSON.stringify(currentAccounts.slice().sort());
        const chainIdChanged = lastKnownChainId !== currentChainId;
        const accountsChanged = lastKnownAccounts !== accountsJson;
        const originsChanged = lastKnownConnectedOrigins !== originsJson;

        // Update state
        lastKnownChainId = currentChainId;
        lastKnownAccounts = accountsJson;
        lastKnownConnectedOrigins = originsJson;

        if (isFirstPoll) {
            isFirstPoll = false;
            return;
        }

        // Notify only relevant tabs
        if (chainIdChanged || accountsChanged || originsChanged) {
            chrome.tabs.query({ url: ["http://*/*", "https://*/*"] }, (tabs) => {
                tabs.forEach(tab => {
                    const tabOrigin = new URL(tab.url).origin;
                    const wasConnected = JSON.parse(lastKnownConnectedOrigins).includes(tabOrigin);
                    const isConnected = currentOrigins.includes(tabOrigin);

                    if (originsChanged) {
                        if (!isConnected && wasConnected) {
                            // Disconnect: Send accountsChanged([])
                            chrome.tabs.sendMessage(tab.id, { type: 'accountsChanged', payload: [] });
                        } else if (isConnected && !wasConnected) {
                            // New connection: Send accountsChanged(current)
                            chrome.tabs.sendMessage(tab.id, { type: 'accountsChanged', payload: currentAccounts });
                        }
                    }

                    if (isConnected && (chainIdChanged || accountsChanged)) {
                        // Global changes only to connected tabs
                        if (chainIdChanged) chrome.tabs.sendMessage(tab.id, { type: 'chainChanged', payload: currentChainId });
                        if (accountsChanged) chrome.tabs.sendMessage(tab.id, { type: 'accountsChanged', payload: currentAccounts });
                    }
                });
            });
        }
    } catch (error) {
        console.error("Background: Error during status poll:", error);
         if (!isFirstPoll && lastKnownAccounts !== JSON.stringify([])) {
             // If we were previously connected and poll fails, assume disconnect
             console.warn("Background: Poll failed, assuming disconnection.");
             lastKnownAccounts = JSON.stringify([]); // Empty accounts
             lastKnownChainId = null; // Reset chain ID
             // TODO: Notify tabs about potential disconnection? (accountsChanged with [])
         }
         isFirstPoll = true; // Reset first poll flag on error? Maybe not.
    }
}


// --- Start Polling ---
setInterval(pollServerStatus, POLLING_INTERVAL_MS);
setTimeout(pollServerStatus, 500);


chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message.target !== 'background') {
        return false;
    }

    // --- Handle Standard Fetch Requests ---
    if (message.type === 'fetch') {
        // console.log(`Background: Received fetch request for ID ${message.id}. Payload URL: '${message.payload.url}'`); // Log with ID
        const { url, options } = message.payload;
        let targetUrl;

        if (url === '/status') targetUrl = SERVER_URL_STATUS;
        else if (url === '/api') targetUrl = SERVER_URL_API;
        else { /* ... error handling ... */ return false; }

        fetch(targetUrl, options)
            .then(response => response.ok ? response.json() : response.text().then(text => { throw new Error(/*...*/) }))
            .then(jsonData => {
                // console.log(`Background: Success fetch for ID ${message.id}. Sending data back.`); // Log success send
                sendResponse({ success: true, data: jsonData });
             })
            .catch(error => {
                 console.error(`Background: Error fetch for ID ${message.id}:`, error); // Log error send
                 sendResponse({ success: false, error: error.message || 'Failed to fetch' });
            });

        return true;
    }

    // --- Handle Connection Confirmation Requests ---
    else if (message.type === 'connection') {
        console.log(`Background: Received connection request ID ${message.id}:`, message.payload);
        const { origin } = message.payload;

        if (!origin) { /* ... error handling ... */ return false; }

        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), CONNECTION_REQUEST_TIMEOUT_MS);

        fetch(SERVER_URL_REQUEST_CONNECTION, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ origin: origin }),
            signal: controller.signal
        })
        .then(async response => {
             clearTimeout(timeoutId);
             if (!response.ok) {
                 const errorText = await response.text().catch(() => `Server returned status ${response.status}`);
                 throw new Error(`Connection request failed: ${errorText}`);
             }
             return response.json();
        })
        .then(serverData => {
             // ***** FIX: Use message.id for logging *****
             console.log(`Background: Received connection response from server for ID ${message.id}:`, serverData);
             if (serverData.status === 'approved') {
                 console.log(`Background: Sending success response for ID ${message.id}`);
                 sendResponse({
                     success: true,
                     data: { approved: true, accounts: serverData.accounts || [] }
                 });
             } else {
                  console.log(`Background: Sending rejection response for ID ${message.id} (status: ${serverData.status})`);
                  sendResponse({ success: false, error: 'User rejected the connection request.' });
             }
        })
        .catch(error => {
            clearTimeout(timeoutId);
             console.error(`Background: Error during connection request ID ${message.id}:`, error);
             let errorMessage = 'Connection to Zeus failed or timed out.';
             if (error.name === 'AbortError') { errorMessage = 'Connection request timed out.'; }
             else if (error.message) { errorMessage = error.message; }

             console.log(`Background: Sending error response for ID ${message.id}`);
             sendResponse({ success: false, error: errorMessage });
        });

        return true;
    }

    return false;
});

console.log('Zeus background service worker started.');