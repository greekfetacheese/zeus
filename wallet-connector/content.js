function injectScript(filePath) {
    try {
        const container = document.head || document.documentElement;
        const scriptTag = document.createElement('script');
        scriptTag.setAttribute('async', 'false'); // Ensure it loads/execs synchronously relative to other scripts if possible
        scriptTag.setAttribute('type', 'text/javascript');
        scriptTag.setAttribute('src', chrome.runtime.getURL(filePath));

        container.insertBefore(scriptTag, container.children[0]); // Inject at the top of head/documentElement
        // Optionally remove the script tag after it has run - cleaner DOM
        // scriptTag.onload = () => { scriptTag.remove(); };
        console.log(`Injected ${filePath}`);

    } catch (error) {
        console.error('Zeus Connector: Error injecting script:', error);
    }
}

// Inject the main provider script
injectScript('injected.js');




// --- Relay messages between injected script and background ---

// 1. Listen for messages FROM the injected script (window.postMessage)
window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data || event.data.target !== 'content') {
        return;
    }

    const message = event.data;
    let messageToBackground = null;

    // Determine message type to send to background
    if (message.type === 'fetch_request') {
        messageToBackground = { target: 'background', type: 'fetch', payload: message.payload };
    } else if (message.type === 'connection_request') {
        messageToBackground = { target: 'background', type: 'connection', payload: message.payload };
    }

    if (messageToBackground) {
        // console.log('Content Script: Forwarding message to background:', messageToBackground);
        // Forward the request to the background script
        chrome.runtime.sendMessage(messageToBackground, (response) => {
            if (chrome.runtime.lastError) {
                console.error('Content Script: Error sending/receiving message:', chrome.runtime.lastError.message);
                // Send error back to injected script (use original request ID)
                window.postMessage({ target: 'injected', type: 'fetch_response', success: false, error: chrome.runtime.lastError.message, id: message.id }, "*");
            } else {
                // console.log('Content Script: Received response from background:', response);
                // Add the original request ID back for correlation
                response.id = message.id;
                // Forward the response back to the injected script
                // Use 'fetch_response' type consistently for simplicity in injected listener
                window.postMessage({ target: 'injected', type: 'fetch_response', ...response }, "*");
            }
        });
    }
});


console.log('Zeus content script loaded and relay listener added.');