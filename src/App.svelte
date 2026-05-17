<script lang="js">
  import './app.css';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  let files = $state([]);
  let processing = $state(false);
  let processedDocuments = $state([]);
  let currentDoc = $state(null);
  let watchFolder = $state('');
  let stats = $state({ total: 0, processed: 0, failed: 0 });
  let dragover = $state(false);
  let errorMsg = $state('');
  let dialogOpen = $state(false);

  onMount(async () => {
    console.log('App mounted, initializing...');

    // Load settings and stats
    try {
      const settings = await invoke('get_settings');
      console.log('Settings loaded:', settings);
      watchFolder = settings.watch_folder || '';
      stats = await invoke('get_stats');
      processedDocuments = await invoke('get_recent_documents', { limit: 10 });
    } catch (e) {
      console.error('Failed to load settings:', e);
      errorMsg = 'Failed to load settings: ' + e;
    }

    // Listen for Tauri drag-drop events (Tauri 2 style)
    try {
      await listen('tauri://drag-over', (event) => {
        console.log('Drag over:', event);
        dragover = true;
      });

      await listen('tauri://drag-drop', async (event) => {
        console.log('File dropped:', event);
        dragover = false;
        const paths = event.payload?.paths || event.payload;
        if (paths && paths.length > 0) {
          files = paths.filter(p =>
            p.toLowerCase().endsWith('.pdf') ||
            p.toLowerCase().endsWith('.docx') ||
            p.toLowerCase().endsWith('.doc') ||
            p.toLowerCase().endsWith('.txt')
          );
          console.log('Filtered files:', files);
          if (files.length > 0) {
            await processFiles();
          }
        }
      });

      await listen('tauri://drag-leave', () => {
        dragover = false;
      });

      console.log('Drag-drop listeners registered');
    } catch (e) {
      console.error('Failed to setup drag-drop:', e);
    }
  });

  async function selectFiles() {
    if (dialogOpen) return;
    dialogOpen = true;
    console.log('selectFiles called');
    errorMsg = '';

    try {
      const selected = await open({
        multiple: true,
        filters: [{
          name: 'Documents',
          extensions: ['pdf', 'docx', 'doc', 'txt']
        }]
      });

      console.log('Dialog result:', selected);

      if (selected) {
        files = Array.isArray(selected) ? selected : [selected];
        await processFiles();
      }
    } catch (e) {
      console.error('Dialog error:', e);
      errorMsg = 'Dialog error: ' + e;
    } finally {
      dialogOpen = false;
    }
  }

  async function selectWatchFolder() {
    if (dialogOpen) return;
    dialogOpen = true;
    console.log('selectWatchFolder called');
    errorMsg = '';
    successMsg = '';

    try {
      const folder = await open({
        directory: true,
        title: 'Wybierz folder z dokumentami'
      });

      console.log('Folder selected:', folder);

      if (folder) {
        watchFolder = folder;
        await invoke('set_watch_folder', { path: folder });

        // Auto-scan the folder
        processing = true;
        const newDocs = await invoke('scan_folder', { path: folder });
        console.log('Scanned documents:', newDocs);

        if (newDocs.length > 0) {
          processedDocuments = [...newDocs, ...processedDocuments];
          stats.total += newDocs.length;
          stats.processed += newDocs.length;
          successMsg = `Przetworzono ${newDocs.length} nowych dokumentów`;
        } else {
          successMsg = 'Folder wybrany. Brak nowych dokumentów do przetworzenia (już przetworzone lub brak PDF/DOCX/TXT)';
        }
        processing = false;
      }
    } catch (e) {
      console.error('Folder dialog error:', e);
      errorMsg = 'Błąd: ' + e;
      processing = false;
    } finally {
      dialogOpen = false;
    }
  }

  async function clearDuplicates() {
    try {
      const deleted = await invoke('clear_duplicates');
      console.log('Deleted duplicates:', deleted);
      // Refresh the document list
      processedDocuments = await invoke('get_recent_documents', { limit: 50 });
      stats = await invoke('get_stats');
    } catch (e) {
      console.error('Clear duplicates error:', e);
      errorMsg = 'Failed to clear duplicates: ' + e;
    }
  }

  let showDeleteConfirm = $state(false);
  let showTypeDropdown = $state(false);

  const docTypes = [
    'unknown', 'umowa', 'faktura', 'regulamin', 'wniosek', 'pismo',
    'ustawa', 'owu', 'pozew', 'wyrok', 'protokół', 'oświadczenie',
    'pełnomocnictwo', 'decyzja', 'raport', 'instrukcja', 'inne'
  ];

  async function deleteAllDocuments() {
    try {
      const deleted = await invoke('delete_all_documents');
      console.log('Deleted all documents:', deleted);
      successMsg = `Usunięto ${deleted} dokumentów z bazy`;
      // Refresh the document list
      processedDocuments = [];
      stats = { total: 0, processed: 0, failed: 0 };
      currentDoc = null;
      showDeleteConfirm = false;
    } catch (e) {
      console.error('Delete all error:', e);
      errorMsg = 'Błąd usuwania: ' + e;
      showDeleteConfirm = false;
    }
  }

  async function changeDocType(docId, newType) {
    try {
      await invoke('update_document_type', { id: docId, docType: newType });
      console.log('Document type changed to:', newType);

      // Update local state
      if (currentDoc && currentDoc.id === docId) {
        currentDoc.doc_type = newType;
        currentDoc.classification_confidence = 1.0;
      }

      // Update in list
      processedDocuments = processedDocuments.map(d =>
        d.id === docId ? { ...d, doc_type: newType, classification_confidence: 1.0 } : d
      );

      successMsg = `Typ dokumentu zmieniony na: ${newType}`;
    } catch (e) {
      console.error('Change type error:', e);
      errorMsg = 'Błąd zmiany typu: ' + e;
    }
  }

  async function rescanFolder() {
    if (!watchFolder || processing) return;
    processing = true;
    errorMsg = '';

    try {
      const newDocs = await invoke('scan_folder_force', { path: watchFolder });
      console.log('Force scanned documents:', newDocs);

      // Refresh list
      processedDocuments = await invoke('get_recent_documents', { limit: 50 });
      stats = await invoke('get_stats');
    } catch (e) {
      console.error('Rescan error:', e);
      errorMsg = 'Rescan error: ' + e;
    } finally {
      processing = false;
    }
  }

  let exportPath = $state('');
  let successMsg = $state('');

  async function exportToJson() {
    if (processing) return;
    if (processedDocuments.length === 0) {
      errorMsg = 'Brak dokumentów do eksportu';
      return;
    }
    processing = true;
    errorMsg = '';
    successMsg = '';

    try {
      console.log('Starting JSON export...');
      exportPath = await invoke('export_to_json');
      console.log('Exported to:', exportPath);
      successMsg = `Wyeksportowano ${processedDocuments.length} dokumentów do: ${exportPath}`;
    } catch (e) {
      console.error('Export error:', e);
      errorMsg = 'Błąd eksportu JSON: ' + e;
    } finally {
      processing = false;
    }
  }

  async function openOriginal(path) {
    try {
      console.log('Opening file:', path);
      await invoke('open_file', { path: path });
    } catch (e) {
      console.error('Open error:', e);
      errorMsg = 'Nie można otworzyć pliku: ' + e;
    }
  }

  async function saveAsMarkdown(doc) {
    try {
      console.log('Exporting to MD:', doc.id);
      errorMsg = '';
      successMsg = '';
      const md = await invoke('export_document_md', { id: doc.id });
      console.log('Saved markdown:', md);
      exportPath = md;
      successMsg = 'Zapisano: ' + md;
    } catch (e) {
      console.error('Markdown export error:', e);
      errorMsg = 'Błąd eksportu MD: ' + e;
    }
  }

  async function printDocument(doc) {
    try {
      console.log('Printing:', doc.id);
      const htmlPath = await invoke('export_document_html', { id: doc.id });
      console.log('HTML exported to:', htmlPath);
      await invoke('open_file', { path: htmlPath });
    } catch (e) {
      console.error('Print error:', e);
      errorMsg = 'Print error: ' + e;
    }
  }

  async function processFiles() {
    if (files.length === 0) return;

    processing = true;
    errorMsg = '';
    console.log('Processing files:', files);

    for (const file of files) {
      try {
        console.log('Processing:', file);
        const result = await invoke('process_document', { path: file });
        console.log('Result:', result);
        processedDocuments = [result, ...processedDocuments];
        stats.processed++;
      } catch (e) {
        console.error('Processing failed:', e);
        errorMsg = 'Processing failed: ' + e;
        stats.failed++;
      }
      stats.total++;
    }

    files = [];
    processing = false;
  }

  async function viewDocument(doc) {
    try {
      currentDoc = await invoke('get_document_details', { id: doc.id });
    } catch (e) {
      console.error('Failed to load document:', e);
      errorMsg = 'Failed to load document: ' + e;
    }
  }

  function getDocTypeColor(type) {
    const colors = {
      'umowa': 'info',
      'pozew': 'warning',
      'ustawa': 'success',
      'unknown': 'secondary'
    };
    return colors[type] || 'info';
  }

  function formatDate(date) {
    if (!date) return 'N/A';
    return new Date(date).toLocaleString('pl-PL');
  }

  function formatSize(bytes) {
    if (!bytes) return 'N/A';
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
    return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
  }
</script>

<main>
  <header>
    <div class="logo">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
        <polyline points="14 2 14 8 20 8"/>
        <line x1="16" y1="13" x2="8" y2="13"/>
        <line x1="16" y1="17" x2="8" y2="17"/>
      </svg>
      <h1>Document Processor</h1>
    </div>
    <div class="stats">
      <div class="stat">
        <span class="value">{stats.total}</span>
        <span class="label">Total</span>
      </div>
      <div class="stat success">
        <span class="value">{stats.processed}</span>
        <span class="label">Processed</span>
      </div>
      <div class="stat error">
        <span class="value">{stats.failed}</span>
        <span class="label">Failed</span>
      </div>
    </div>
  </header>

  {#if errorMsg}
    <div class="error-banner">{errorMsg}</div>
  {/if}

  {#if successMsg}
    <div class="success-banner">{successMsg}</div>
  {/if}

  <div class="content">
    <aside class="sidebar">
      <div class="card">
        <h3>Import dokumentów</h3>
        <p class="folder-path">{watchFolder || 'Nie wybrano folderu'}</p>
        <button class="secondary" onclick={selectWatchFolder} style="width: 100%;">
          Wybierz folder
        </button>
        {#if watchFolder}
          <button class="secondary" onclick={rescanFolder} style="margin-top: 8px; width: 100%;">
            Skanuj ponownie
          </button>
        {/if}
      </div>

      <div class="card">
        <h3>Recent Documents ({processedDocuments.length})</h3>
        <ul class="doc-list">
          {#each processedDocuments as doc}
            <li onclick={() => viewDocument(doc)} class:active={currentDoc?.id === doc.id}>
              <span class="doc-name">{doc.filename}</span>
              <span class="badge {getDocTypeColor(doc.doc_type)}">{doc.doc_type || 'unknown'}</span>
            </li>
          {/each}
          {#if processedDocuments.length === 0}
            <li class="empty">No documents yet</li>
          {/if}
        </ul>
        {#if processedDocuments.length > 0}
          <button class="secondary" onclick={exportToJson} style="margin-top: 12px; width: 100%;">
            📤 Eksportuj do JSON
          </button>
          <button class="secondary danger" onclick={clearDuplicates} style="margin-top: 8px; width: 100%;">
            Wyczyść duplikaty
          </button>
          <button class="secondary danger" onclick={() => showDeleteConfirm = true} style="margin-top: 8px; width: 100%;">
            🗑️ Usuń wszystkie
          </button>
        {/if}
      </div>
    </aside>

    <section class="main-area">
      {#if !currentDoc}
        <div
          class="drop-zone"
          class:dragover
          role="button"
          tabindex="0"
          onclick={selectFiles}
          onkeydown={(e) => e.key === 'Enter' && selectFiles()}
        >
          {#if processing}
            <div class="spinner"></div>
            <p>Processing documents...</p>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="upload-icon">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
              <polyline points="17 8 12 3 7 8"/>
              <line x1="12" y1="3" x2="12" y2="15"/>
            </svg>
            <h2>Drop documents here</h2>
            <p>or click to browse</p>
            <p class="supported">Supported: PDF, DOCX, DOC, TXT, RTF</p>
          {/if}
        </div>
      {:else}
        <div class="document-view">
          <div class="doc-header">
            <button class="secondary" onclick={() => currentDoc = null}>
              &larr; Wróć
            </button>
            <h2>{currentDoc.filename}</h2>
            <div class="custom-dropdown">
              <button
                class="dropdown-trigger {getDocTypeColor(currentDoc.doc_type)}"
                onclick={() => showTypeDropdown = !showTypeDropdown}
              >
                {currentDoc.doc_type || 'unknown'}
                <span class="dropdown-arrow">▼</span>
              </button>
              {#if showTypeDropdown}
                <div class="dropdown-menu">
                  {#each docTypes as dtype}
                    <button
                      class="dropdown-item"
                      class:active={currentDoc.doc_type === dtype}
                      onclick={() => {
                        changeDocType(currentDoc.id, dtype);
                        showTypeDropdown = false;
                      }}
                    >
                      {dtype}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          </div>

          <div class="doc-actions">
            <button class="secondary" onclick={() => openOriginal(currentDoc.original_path)}>
              📄 Otwórz oryginał
            </button>
            <button class="secondary" onclick={() => saveAsMarkdown(currentDoc)}>
              📝 Zapisz jako .md
            </button>
            <button class="secondary" onclick={() => printDocument(currentDoc)}>
              🖨️ Drukuj
            </button>
          </div>

          <div class="doc-meta">
            <div class="meta-item">
              <span class="label">Pages</span>
              <span class="value">{currentDoc.pages || 'N/A'}</span>
            </div>
            <div class="meta-item">
              <span class="label">Words</span>
              <span class="value">{currentDoc.word_count || 'N/A'}</span>
            </div>
            <div class="meta-item">
              <span class="label">Images</span>
              <span class="value">{currentDoc.images?.length || 0}</span>
            </div>
            <div class="meta-item">
              <span class="label">Size</span>
              <span class="value">{formatSize(currentDoc.size)}</span>
            </div>
            <div class="meta-item">
              <span class="label">Processed</span>
              <span class="value">{formatDate(currentDoc.processed_at)}</span>
            </div>
          </div>

          <div class="doc-content">
            <h3>Pełna treść dokumentu</h3>
            <div class="text-preview">
              {currentDoc.full_text || currentDoc.text_preview || 'No text extracted'}
            </div>
          </div>

          {#if currentDoc.images?.length > 0}
            <div class="images-section">
              <h3>Extracted Images ({currentDoc.images.length})</h3>
              <div class="image-grid">
                {#each currentDoc.images as img}
                  <div class="image-card">
                    <div class="image-placeholder">Image: {img.filename}</div>
                    <div class="image-meta">
                      {#if img.context_before}
                        <p><strong>Before:</strong> ...{img.context_before.slice(-100)}</p>
                      {/if}
                      {#if img.ai_description}
                        <p><strong>AI:</strong> {img.ai_description}</p>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}
    </section>
  </div>

  {#if showDeleteConfirm}
    <div class="modal-overlay" onclick={() => showDeleteConfirm = false}>
      <div class="modal" onclick={(e) => e.stopPropagation()}>
        <h3>⚠️ Potwierdzenie usunięcia</h3>
        <p>Czy na pewno chcesz usunąć <strong>wszystkie dokumenty</strong> z bazy danych?</p>
        <p class="warning-text">Ta operacja jest nieodwracalna!</p>
        <div class="modal-actions">
          <button class="secondary" onclick={() => showDeleteConfirm = false}>
            Anuluj
          </button>
          <button class="danger" onclick={deleteAllDocuments}>
            🗑️ Usuń wszystkie
          </button>
        </div>
      </div>
    </div>
  {/if}
</main>

