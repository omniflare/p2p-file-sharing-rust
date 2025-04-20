document.addEventListener('DOMContentLoaded', () => {
    const dropArea = document.getElementById('drop-area');
    const fileInput = document.getElementById('file-input');
    const fileName = document.getElementById('file-name');
    const shareLinkContainer = document.getElementById('share-link-container');
    const shareLink = document.getElementById('share-link');
    const copyLink = document.getElementById('copy-link');
    const transferStatus = document.getElementById('transfer-status');
    const progressBar = document.getElementById('progress-bar');
    const statusText = document.getElementById('status-text');
    
    let selectedFile = null;
    let ws = null;
    let transferId = null;
    let receiverId = null;
    let chunkSize = 64 * 1024; // 64KB chunks
    
    // Connect to WebSocket
    function connectWebSocket() {
        ws = new WebSocket(`${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`);
        
        ws.onopen = () => {
            console.log('WebSocket connected');
        };
        
        ws.onmessage = (event) => {
            try {
                if (typeof event.data === 'string') {
                    const data = JSON.parse(event.data);
                    
                    if (data.type === 'transfer_created') {
                        transferId = data.transfer_id;
                        const fullUrl = `${window.location.origin}${data.share_url}`;
                        shareLink.value = fullUrl;
                        shareLinkContainer.classList.remove('hidden');
                    }
                    
                    if (data.type === 'receiver_connected') {
                        receiverId = data.receiver_id;
                        transferStatus.classList.remove('hidden');
                        statusText.textContent = 'Recipient connected! Starting transfer...';
                        
                        // Start sending the file
                        sendFile();
                    }
                }
            } catch (error) {
                console.error('Error processing message:', error);
            }
        };
        
        ws.onclose = () => {
            console.log('WebSocket disconnected');
        };
        
        ws.onerror = (error) => {
            console.error('WebSocket error:', error);
        };
    }
    
    // Initialize file transfer
    function initTransfer() {
        if (!selectedFile || !ws) return;
        
        ws.send(JSON.stringify({
            type: 'init_transfer',
            file_name: selectedFile.name,
            file_size: selectedFile.size
        }));
    }
    
    // Send file in chunks
    async function sendFile() {
        if (!selectedFile || !receiverId) return;
        
        const totalChunks = Math.ceil(selectedFile.size / chunkSize);
        let currentChunk = 0;
        
        // Send metadata first
        ws.send(JSON.stringify({
            type: 'file_metadata',
            target_id: receiverId,
            file_name: selectedFile.name,
            file_size: selectedFile.size,
            total_chunks: totalChunks
        }));
        
        // Read and send file chunks
        for (let start = 0; start < selectedFile.size; start += chunkSize) {
            const end = Math.min(start + chunkSize, selectedFile.size);
            const chunk = selectedFile.slice(start, end);
            const buffer = await chunk.arrayBuffer();
            
            // Wait for WebSocket to be ready
            if (ws.readyState === WebSocket.OPEN) {
                // First send chunk metadata
                ws.send(JSON.stringify({
                    type: 'chunk_metadata',
                    target_id: receiverId,
                    chunk_index: currentChunk,
                    chunk_size: end - start
                }));
                
                // Then send the actual chunk data
                ws.send(buffer);
                
                // Update progress
                currentChunk++;
                const progress = Math.round((currentChunk / totalChunks) * 100);
                progressBar.style.width = `${progress}%`;
                statusText.textContent = `Sending: ${progress}% (${currentChunk} of ${totalChunks} chunks)`;
                
                // Small delay to prevent flooding
                await new Promise(resolve => setTimeout(resolve, 10));
            } else {
                statusText.textContent = 'Connection lost. Please try again.';
                break;
            }
        }
        
        if (currentChunk === totalChunks) {
            // Send transfer complete message
            ws.send(JSON.stringify({
                type: 'transfer_complete',
                target_id: receiverId,
            }));
            
            statusText.textContent = 'Transfer complete!';
        }
    }
    
    // Initialize
    connectWebSocket();
    
    // Set up file input change event
    fileInput.addEventListener('change', (event) => {
        if (event.target.files.length > 0) {
            selectedFile = event.target.files[0];
            fileName.textContent = `Selected: ${selectedFile.name} (${formatFileSize(selectedFile.size)})`;
            
            // Initialize transfer when file is selected
            initTransfer();
        }
    });
    
    // Set up drag and drop events
    ['dragenter', 'dragover', 'dragleave', 'drop'].forEach(eventName => {
        dropArea.addEventListener(eventName, (e) => {
            e.preventDefault();
            e.stopPropagation();
        }, false);
    });
    
    // Highlight drop area when file is dragged over
    ['dragenter', 'dragover'].forEach(eventName => {
        dropArea.addEventListener(eventName, () => {
            dropArea.classList.add('active');
        }, false);
    });
    
    ['dragleave', 'drop'].forEach(eventName => {
        dropArea.addEventListener(eventName, () => {
            dropArea.classList.remove('active');
        }, false);
    });
    
    // Handle file drop
    dropArea.addEventListener('drop', (e) => {
        if (e.dataTransfer.files.length > 0) {
            selectedFile = e.dataTransfer.files[0];
            fileName.textContent = `Selected: ${selectedFile.name} (${formatFileSize(selectedFile.size)})`;
            
            // Initialize transfer when file is dropped
            initTransfer();
        }
    }, false);
    
    // Copy link button
    copyLink.addEventListener('click', () => {
        shareLink.select();
        document.execCommand('copy');
        copyLink.textContent = 'Copied!';
        setTimeout(() => {
            copyLink.textContent = 'Copy';
        }, 2000);
    });
    
    function formatFileSize(bytes) {
        if (bytes === 0) return '0 Bytes';
        
        const k = 1024;
        const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        
        return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
    }
    
    // Handle page unload to clean up WebSocket
    window.addEventListener('beforeunload', () => {
        if (ws && ws.readyState === WebSocket.OPEN) {
            ws.close();
        }
    });
});
