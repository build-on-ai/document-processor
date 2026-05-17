#!/usr/bin/env python3
"""
Document Upload Tester v2.0
Based on document-upload-analyzer skill - tests ALL known upload methods.

Categories tested:
1. Direct User Upload (file picker, drag&drop, paste, camera)
2. Bulk Operations (ZIP, CSV with URLs, folder upload)
3. External Sources (cloud storage, URL import, API)
4. Communication Channels (email, FTP, WebDAV)
5. Automated (cron/watch folder, OCR)
6. Programmatic (REST API, CLI)
7. Administrative (filesystem copy, database import)
"""

import os
import sys
import json
import subprocess
import tempfile
import shutil
import zipfile
import http.server
import socketserver
import threading
import time
import csv
from pathlib import Path
from datetime import datetime
from dataclasses import dataclass, asdict
from typing import List, Optional
import urllib.request

@dataclass
class TestResult:
    category: str
    test_name: str
    description: str
    status: str  # PASS, FAIL, MANUAL, SKIP, N/A
    details: Optional[str] = None
    instructions: Optional[str] = None
    priority: str = "MEDIUM"  # HIGH, MEDIUM, LOW

class DocumentUploadTester:
    """Comprehensive document upload tester based on document-upload-analyzer skill"""

    def __init__(self, app_dir: Path):
        self.app_dir = app_dir
        self.test_dir = Path(tempfile.mkdtemp(prefix="upload_test_"))
        self.results: List[TestResult] = []
        self.sample_files = {}
        self.http_server = None
        self.http_port = 8765

    def setup(self):
        """Setup test environment and sample files"""
        print("=" * 70)
        print("DOCUMENT UPLOAD TESTER v2.0")
        print("Based on document-upload-analyzer skill")
        print("=" * 70)
        print(f"\nTest directory: {self.test_dir}")

        # Create directories
        (self.test_dir / "input").mkdir()
        (self.test_dir / "watch_folder").mkdir()
        (self.test_dir / "bulk").mkdir()
        (self.test_dir / "output").mkdir()

        # Create sample files
        self._create_sample_pdf()
        self._create_sample_txt()
        self._create_sample_docx()
        self._create_bulk_zip()
        self._create_url_csv()

        print(f"Sample files created: {list(self.sample_files.keys())}")

    def _create_sample_pdf(self):
        """Create minimal valid PDF"""
        pdf_content = b"""%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >> endobj
4 0 obj << /Length 55 >> stream
BT /F1 12 Tf 100 700 Td (Test PDF - Umowa najmu lokalu) Tj ET
endstream endobj
xref
0 5
0000000000 65535 f
0000000009 00000 n
0000000058 00000 n
0000000115 00000 n
0000000214 00000 n
trailer << /Size 5 /Root 1 0 R >>
startxref
318
%%EOF"""
        path = self.test_dir / "input" / "test.pdf"
        path.write_bytes(pdf_content)
        self.sample_files['pdf'] = path

    def _create_sample_txt(self):
        """Create sample TXT file"""
        content = """UMOWA NAJMU LOKALU MIESZKALNEGO

Zawarta w dniu 20 grudnia 2025 r. w Warszawie

STRONY UMOWY:
1. Wynajmujący: Jan Kowalski
2. Najemca: Anna Nowak

PRZEDMIOT UMOWY:
Lokal mieszkalny przy ul. Testowej 123, Warszawa.

WARUNKI:
- Czynsz: 3000 PLN miesięcznie
- Okres: 12 miesięcy
"""
        path = self.test_dir / "input" / "test.txt"
        path.write_text(content, encoding='utf-8')
        self.sample_files['txt'] = path

    def _create_sample_docx(self):
        """Create minimal DOCX"""
        content_types = '''<?xml version="1.0"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>'''

        rels = '''<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>'''

        document = '''<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>Test DOCX - Pozew o zapłatę</w:t></w:r></w:p></w:body>
</w:document>'''

        path = self.test_dir / "input" / "test.docx"
        with zipfile.ZipFile(path, 'w') as zf:
            zf.writestr('[Content_Types].xml', content_types)
            zf.writestr('_rels/.rels', rels)
            zf.writestr('word/document.xml', document)
        self.sample_files['docx'] = path

    def _create_bulk_zip(self):
        """Create ZIP with multiple documents"""
        zip_path = self.test_dir / "bulk" / "documents.zip"
        with zipfile.ZipFile(zip_path, 'w') as zf:
            zf.write(self.sample_files['pdf'], "doc1.pdf")
            zf.write(self.sample_files['txt'], "doc2.txt")
        self.sample_files['zip'] = zip_path

    def _create_url_csv(self):
        """Create CSV with document URLs"""
        csv_path = self.test_dir / "bulk" / "documents.csv"
        with open(csv_path, 'w', newline='') as f:
            writer = csv.writer(f)
            writer.writerow(['name', 'url', 'type'])
            writer.writerow(['sample.pdf', f'http://localhost:{self.http_port}/test.pdf', 'pdf'])
        self.sample_files['csv'] = csv_path

    # =========================================================================
    # CATEGORY 1: DIRECT USER UPLOAD
    # =========================================================================

    def test_category_1_direct_upload(self):
        """Test direct user upload methods"""
        print("\n" + "=" * 70)
        print("CATEGORY 1: DIRECT USER UPLOAD")
        print("=" * 70)

        # 1.1 Single file picker
        self.results.append(TestResult(
            category="1_direct_upload",
            test_name="single_file_picker",
            description="Single file selection via native dialog",
            status="MANUAL",
            priority="HIGH",
            instructions=f"""
1. Click on the drop zone in Document Processor
2. Select file: {self.sample_files['pdf']}
3. Verify file is processed and appears in list
"""
        ))

        # 1.2 Multi-file picker
        self.results.append(TestResult(
            category="1_direct_upload",
            test_name="multi_file_picker",
            description="Multiple file selection",
            status="MANUAL",
            priority="HIGH",
            instructions=f"""
1. Click on drop zone
2. Select multiple files: {self.sample_files['pdf']}, {self.sample_files['txt']}
3. Verify all files are processed
"""
        ))

        # 1.3 Drag & Drop single file
        self.results.append(TestResult(
            category="1_direct_upload",
            test_name="drag_drop_single",
            description="Drag & drop single file",
            status="MANUAL",
            priority="HIGH",
            instructions=f"""
1. Open file manager
2. Drag {self.sample_files['pdf']} to Document Processor window
3. Drop zone should highlight on hover
4. File should be processed after drop
"""
        ))

        # 1.4 Drag & Drop multiple files
        self.results.append(TestResult(
            category="1_direct_upload",
            test_name="drag_drop_multi",
            description="Drag & drop multiple files",
            status="MANUAL",
            priority="HIGH",
            instructions=f"""
1. Select multiple files in file manager
2. Drag all to Document Processor
3. All should be processed
"""
        ))

        # 1.5 Paste from clipboard
        self.results.append(TestResult(
            category="1_direct_upload",
            test_name="clipboard_paste",
            description="Paste file from clipboard (Ctrl+V)",
            status="N/A",
            priority="MEDIUM",
            details="Not implemented - requires clipboard file API"
        ))

        # 1.6 Camera/Scanner input
        self.results.append(TestResult(
            category="1_direct_upload",
            test_name="camera_scanner",
            description="Direct camera/scanner capture",
            status="N/A",
            priority="LOW",
            details="Not implemented - would require camera API integration"
        ))

    # =========================================================================
    # CATEGORY 2: BULK OPERATIONS
    # =========================================================================

    def test_category_2_bulk_operations(self):
        """Test bulk upload methods"""
        print("\n" + "=" * 70)
        print("CATEGORY 2: BULK OPERATIONS")
        print("=" * 70)

        # 2.1 ZIP archive upload
        self.results.append(TestResult(
            category="2_bulk",
            test_name="zip_upload",
            description="Upload ZIP archive with documents",
            status="MANUAL",
            priority="MEDIUM",
            instructions=f"""
1. Upload: {self.sample_files['zip']}
2. Expected: ZIP should be extracted and each document processed
Current status: Not implemented (processes ZIP as single file)
"""
        ))

        # 2.2 CSV with URLs
        self.results.append(TestResult(
            category="2_bulk",
            test_name="csv_url_import",
            description="Import documents from CSV with URLs",
            status="N/A",
            priority="MEDIUM",
            details=f"CSV prepared: {self.sample_files['csv']} - requires URL import feature"
        ))

        # 2.3 Folder upload
        self.results.append(TestResult(
            category="2_bulk",
            test_name="folder_upload",
            description="Upload entire folder recursively",
            status="MANUAL",
            priority="MEDIUM",
            instructions=f"""
1. Select folder via "Change Folder" button
2. All documents in folder should be listed
Test folder: {self.test_dir / 'input'}
"""
        ))

    # =========================================================================
    # CATEGORY 3: EXTERNAL SOURCES
    # =========================================================================

    def test_category_3_external_sources(self):
        """Test external source imports"""
        print("\n" + "=" * 70)
        print("CATEGORY 3: EXTERNAL SOURCES")
        print("=" * 70)

        external_sources = [
            ("google_drive", "Google Drive integration", "N/A"),
            ("dropbox", "Dropbox integration", "N/A"),
            ("onedrive", "OneDrive integration", "N/A"),
            ("url_import", "Download from URL", "N/A"),
            ("api_webhook", "Receive via webhook/API", "N/A"),
        ]

        for name, desc, status in external_sources:
            self.results.append(TestResult(
                category="3_external",
                test_name=name,
                description=desc,
                status=status,
                priority="LOW",
                details="Cloud integration not implemented"
            ))

    # =========================================================================
    # CATEGORY 4: COMMUNICATION CHANNELS
    # =========================================================================

    def test_category_4_communication(self):
        """Test communication channel imports"""
        print("\n" + "=" * 70)
        print("CATEGORY 4: COMMUNICATION CHANNELS")
        print("=" * 70)

        channels = [
            ("email_imap", "Import from email (IMAP)", "N/A"),
            ("ftp_sftp", "FTP/SFTP upload", "N/A"),
            ("webdav", "WebDAV mount", "N/A"),
            ("slack", "Slack attachment import", "N/A"),
        ]

        for name, desc, status in channels:
            self.results.append(TestResult(
                category="4_communication",
                test_name=name,
                description=desc,
                status=status,
                priority="LOW",
                details="Communication channel not implemented"
            ))

    # =========================================================================
    # CATEGORY 5: AUTOMATED
    # =========================================================================

    def test_category_5_automated(self):
        """Test automated upload methods"""
        print("\n" + "=" * 70)
        print("CATEGORY 5: AUTOMATED")
        print("=" * 70)

        # 5.1 Watch folder
        self.results.append(TestResult(
            category="5_automated",
            test_name="watch_folder",
            description="Auto-process files added to watch folder",
            status="MANUAL",
            priority="HIGH",
            instructions=f"""
1. Set watch folder to: {self.test_dir / 'watch_folder'}
2. Copy a PDF to that folder
3. Document should be auto-processed

Test command:
cp {self.sample_files['pdf']} {self.test_dir / 'watch_folder'}/
"""
        ))

        # 5.2 OCR from images
        self.results.append(TestResult(
            category="5_automated",
            test_name="ocr_images",
            description="OCR text extraction from scanned images",
            status="MANUAL",
            priority="MEDIUM",
            details="OCR is attempted on PDF images during processing"
        ))

    # =========================================================================
    # CATEGORY 6: PROGRAMMATIC
    # =========================================================================

    def test_category_6_programmatic(self):
        """Test programmatic upload methods"""
        print("\n" + "=" * 70)
        print("CATEGORY 6: PROGRAMMATIC")
        print("=" * 70)

        # 6.1 CLI
        self.results.append(TestResult(
            category="6_programmatic",
            test_name="cli_tool",
            description="Command-line document processing",
            status="N/A",
            priority="MEDIUM",
            details="CLI interface not yet exposed - Tauri commands only"
        ))

        # 6.2 REST API
        self.results.append(TestResult(
            category="6_programmatic",
            test_name="rest_api",
            description="REST API for document upload",
            status="N/A",
            priority="MEDIUM",
            details="HTTP API not implemented - only Tauri IPC"
        ))

    # =========================================================================
    # CATEGORY 7: ADMINISTRATIVE
    # =========================================================================

    def test_category_7_administrative(self):
        """Test administrative upload methods"""
        print("\n" + "=" * 70)
        print("CATEGORY 7: ADMINISTRATIVE")
        print("=" * 70)

        # 7.1 Filesystem copy
        self.results.append(TestResult(
            category="7_admin",
            test_name="filesystem_copy",
            description="Direct filesystem copy to data directory",
            status="MANUAL",
            priority="LOW",
            instructions="""
1. Find app data directory (~/.local/share/com.buildonai.document-processor/)
2. Copy files directly to processed/ folder
3. Check if app recognizes them on restart
"""
        ))

        # 7.2 Database import
        self.results.append(TestResult(
            category="7_admin",
            test_name="database_import",
            description="Import records directly to SQLite",
            status="MANUAL",
            priority="LOW",
            details="Database: ~/.local/share/com.buildonai.document-processor/documents.db"
        ))

    def run_all_tests(self):
        """Run all test categories"""
        self.setup()

        self.test_category_1_direct_upload()
        self.test_category_2_bulk_operations()
        self.test_category_3_external_sources()
        self.test_category_4_communication()
        self.test_category_5_automated()
        self.test_category_6_programmatic()
        self.test_category_7_administrative()

        return self.generate_report()

    def generate_report(self):
        """Generate comprehensive test report"""
        # Count by status
        status_counts = {}
        for r in self.results:
            status_counts[r.status] = status_counts.get(r.status, 0) + 1

        # Count by category
        category_counts = {}
        for r in self.results:
            cat = r.category
            if cat not in category_counts:
                category_counts[cat] = {"total": 0, "implemented": 0}
            category_counts[cat]["total"] += 1
            if r.status not in ["N/A", "SKIP"]:
                category_counts[cat]["implemented"] += 1

        report = {
            "title": "Document Upload Tester v2.0 Report",
            "based_on": "document-upload-analyzer skill",
            "timestamp": datetime.now().isoformat(),
            "test_directory": str(self.test_dir),
            "sample_files": {k: str(v) for k, v in self.sample_files.items()},
            "summary": {
                "total_tests": len(self.results),
                "by_status": status_counts,
                "by_category": category_counts
            },
            "results": [asdict(r) for r in self.results]
        }

        # Save JSON report
        report_path = self.test_dir / "upload_test_report.json"
        with open(report_path, 'w') as f:
            json.dump(report, f, indent=2)

        # Print summary
        print("\n" + "=" * 70)
        print("TEST SUMMARY")
        print("=" * 70)
        print(f"Total tests: {len(self.results)}")
        for status, count in sorted(status_counts.items()):
            print(f"  {status}: {count}")

        print("\n" + "-" * 70)
        print("BY CATEGORY:")
        print("-" * 70)
        for cat, counts in sorted(category_counts.items()):
            impl = counts["implemented"]
            total = counts["total"]
            pct = (impl / total * 100) if total > 0 else 0
            print(f"  {cat}: {impl}/{total} implemented ({pct:.0f}%)")

        # Print manual tests
        manual_tests = [r for r in self.results if r.status == "MANUAL"]
        if manual_tests:
            print("\n" + "=" * 70)
            print(f"MANUAL TESTS TO RUN ({len(manual_tests)})")
            print("=" * 70)
            for r in manual_tests:
                print(f"\n[{r.category}] {r.test_name}")
                print(f"  Description: {r.description}")
                print(f"  Priority: {r.priority}")
                if r.instructions:
                    print(f"  Instructions:\n{r.instructions}")

        print(f"\nFull report: {report_path}")
        print(f"Test files: {self.test_dir}")

        return report

def main():
    app_dir = Path.home() / "projects/buildonai/document-processor"
    tester = DocumentUploadTester(app_dir)
    report = tester.run_all_tests()
    return 0

if __name__ == "__main__":
    sys.exit(main())
