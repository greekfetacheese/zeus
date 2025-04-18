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





// Listen for messages FROM the injected script (window.postMessage)
window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data || event.data.target !== 'content') {
        return;
    }

    const message = event.data;
    let messageToBackground = null;

    if (message.type === 'fetch_request') {
        messageToBackground = { target: 'background', type: 'fetch', payload: message.payload };
    } else if (message.type === 'connection_request') {
        messageToBackground = { target: 'background', type: 'connection', payload: message.payload };
    }

    if (messageToBackground) {
        chrome.runtime.sendMessage(messageToBackground, (response) => {
            if (chrome.runtime.lastError) {
                console.error('Content Script: Error sending/receiving message:', chrome.runtime.lastError.message);
                window.postMessage({ target: 'injected', type: 'fetch_response', success: false, error: chrome.runtime.lastError.message, id: message.id }, "*");
            } else {
                response.id = message.id;
                window.postMessage({ target: 'injected', type: 'fetch_response', ...response }, "*");
            }
        });
    }
});


// ***** Listen for messages FROM background script *****
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message.type === 'accountsChanged' || message.type === 'chainChanged') {
        console.log(`Content Script: Received ${message.type} from background. Relaying to injected script.`);
        window.postMessage({
            target: 'injected',
            type: message.type,
            payload: message.payload
        }, "*");
    }
    return false;
});


console.log('Zeus content script loaded and relay listener added.');