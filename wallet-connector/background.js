// background.js (Simplified)

const SERVER_PORT = 65534;
const SERVER_URL_STATUS = `http://127.0.0.1:${SERVER_PORT}/status`;
const SERVER_URL_API = `http://127.0.0.1:${SERVER_PORT}/api`;
const SERVER_URL_REQUEST_CONNECTION = `http://127.0.0.1:${SERVER_PORT}/request-connection`;

const CONNECTION_REQUEST_TIMEOUT_MS = 30000; // 30 seconds

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
                 console.log(`Background: Sending success response for ID ${message.id}`); // Log success send
                 sendResponse({
                     success: true,
                     data: { approved: true, accounts: serverData.accounts || [] }
                 });
             } else {
                  console.log(`Background: Sending rejection response for ID ${message.id} (status: ${serverData.status})`); // Log reject send
                  sendResponse({ success: false, error: 'User rejected the connection request.' }); // No 'approved' field needed on rejection
             }
        })
        .catch(error => {
            clearTimeout(timeoutId);
             // ***** FIX: Use message.id for logging *****
             console.error(`Background: Error during connection request ID ${message.id}:`, error);
             let errorMessage = 'Connection to Zeus failed or timed out.';
             if (error.name === 'AbortError') { errorMessage = 'Connection request timed out.'; }
             else if (error.message) { errorMessage = error.message; }

             console.log(`Background: Sending error response for ID ${message.id}`); // Log error send
             sendResponse({ success: false, error: errorMessage }); // No 'approved' field needed on error
        });

        return true; // Indicate async response
    }

    return false;
});

console.log('Zeus background service worker started.');