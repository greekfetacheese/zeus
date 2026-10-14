function injectScript(filePath) {
    try {
        let container = document.head || document.documentElement;
        if (!container) {
            // If head isn't ready, wait briefly and retry (for document_start edge cases)
            setTimeout(() => injectScript(filePath), 0);
            return;
        }
        const scriptTag = document.createElement('script');
        scriptTag.setAttribute('type', 'text/javascript');
        scriptTag.setAttribute('src', chrome.runtime.getURL(filePath));
        container.insertBefore(scriptTag, container.firstChild);  // Top of head
        scriptTag.onload = () => { scriptTag.remove(); };  // Clean up
        // console.log(`Injected ${filePath}`);
    } catch (error) {
        console.error('Zeus Connector: Error injecting script:', error);
    }
}

// Inject the main provider script
injectScript('injected.js');


// Connector paths the page-world provider is allowed to reach. The provider only
// ever calls backgroundFetch('/api', ...) (injected.js), so this is the whole list.
// Anything else the page posts is refused here rather than relayed: /status in
// particular carries the connected-dapp origin list and needs no origin, so a
// relaying bridge would let any site enumerate the user's connected dapps.
const ALLOWED_FETCH_URLS = new Set(['/api']);


// Listen for messages FROM the injected script (window.postMessage)
window.addEventListener("message", (event) => {
    if (event.source !== window || !event.data || event.data.target !== 'content') {
        return;
    }

    const message = event.data;
    let messageToBackground = null;

    if (message.type === 'fetch_request') {
        const url = message.payload && message.payload.url;
        if (!ALLOWED_FETCH_URLS.has(url)) {
            // Same failure shape the page already handles for a failed relay,
            // so the provider's pending request rejects instead of hanging.
            window.postMessage({ target: 'injected', type: 'fetch_response', success: false, error: `Blocked connector path: ${url}`, id: message.id }, "*");
            return;
        }
        messageToBackground = { target: 'background', type: 'fetch', payload: message.payload };
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
        // console.log(`Content Script: Received ${message.type} from background. Relaying to injected script.`);
        window.postMessage({
            target: 'injected',
            type: message.type,
            payload: message.payload
        }, "*");
    }
    return false;
});


console.log('Zeus content script loaded and relay listener added.');