document.addEventListener('DOMContentLoaded', () => {
    const startTransferBtn = document.getElementById('start-transfer');
    const transferStatus = document.getElementById('transfer-status');
    const progressBar = document.getElementById('progress-bar');
    const statusText = document.getElementById('status-text');
    const downloadContainer = document.getElementById('download-container');
    const downloadLink = document.getElementById('download-link');
    
    // File data variables
    let ws = null;
    let fileMetadata = null;
    let receivedChunks = [];
    let receivedSize = 0;
    let totalSize = 0;
    
    // Connect to WebSocket with transfer ID
    function connectWebSocket() {
        ws = new WebSocket(`${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws/${TRANSFER_ID}`);
        
        ws.onopen = () => {
            console.log('WebSocket connected');
            transferStatus.classList.remove('hidden');
        };
        
        ws.onmessage = async (event) => {
            try {
                // Handle text messages (metadata)
                if (typeof event.data === 'string') {
                    const data = JSON.parse(event.data);
                    
                    if (data.type === 'file_metadata') {
                        fileMetadata = {
                            name: data.file_name,
                            size: data.file_size,
                            totalChunks: data.total_chunks
                        };
                        
                        // Initialize array to store chunks
                        receivedChunks = new Array(fileMetadata.totalChunks);
                        totalSize = fileMetadata.size;
                        statusText.textContent = `Starting transfer of ${fileMetadata.name}...`;
                    }
                    
                    if (data.type === 'chunk_metadata') {
                        // The next message after this will be the binary chunk data
                        statusText.textContent = `Receiving: ${Math.round((receivedSize / totalSize) * 100)}%`;
                    }
                    
                    if (data.type === 'transfer_complete') {
                        // All chunks received, assemble the file
                        assembleFile();
                    }
                }
                // Handle binary messages (file chunks)
                else if (event.data instanceof Blob) {
                    if (!fileMetadata) return;
                    
                    // Store the received chunk
                    const arrayBuffer = await event.data.arrayBuffer();
                    const lastChunkIndex = receivedChunks.findIndex(chunk => chunk === undefined);
                    
                    if (lastChunkIndex !== -1) {
                        receivedChunks[lastChunkIndex] = arrayBuffer;
                        receivedSize += arrayBuffer.byteLength;
                        
                        // Update progress
                        const progress = Math.round((receivedSize / totalSize) * 100);
                        progressBar.style.width = `${progress}%`;
                        statusText.textContent = `Receiving: ${progress}%`;
                    }
                }
            } catch (error) {
                console.error('Error processing message:', error);
                statusText.textContent = 'Error receiving file. Please try again.';
            }
        };
        
        ws.onclose = () => {
            console.log('WebSocket disconnected');
        };
        
        ws.onerror = (error) => {
            console.error('WebSocket error:', error);
            statusText.textContent = 'Connection error. Please try again.';
        };
    }
    
    // Assemble file chunks and create download link
    function assembleFile() {
        if (!fileMetadata || receivedChunks.some(chunk => chunk === undefined)) {
            statusText.textContent = 'Error: Some chunks are missing. Please try again.';
            return;
        }
        
        // Combine all chunks into one Blob
        const fileBlob = new Blob(receivedChunks, { type: 'application/octet-stream' });
        
        // Create download URL
        const downloadUrl = URL.createObjectURL(fileBlob);
        downloadLink.href = downloadUrl;
        downloadLink.download = fileMetadata.name;
        
        // Show download section
        statusText.textContent = 'Transfer complete!';
        downloadContainer.classList.remove('hidden');
    }
    
    // Start transfer button
    startTransferBtn.addEventListener('click', () => {
        startTransferBtn.disabled = true;
        startTransferBtn.textContent = 'Connecting...';
        connectWebSocket();
    });
    
    // Handle page unload to clean up WebSocket
    window.addEventListener('beforeunload', () => {
        if (ws && ws.readyState === WebSocket.OPEN) {
            ws.close();
        }
    });
});