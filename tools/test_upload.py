#!/usr/bin/env python3
"""
Document Processor Upload Tester v2.0
Tests all upload/import methods for Document Processor application.
"""

import os
import sys
import json
import subprocess
import tempfile
import shutil
from pathlib import Path
from datetime import datetime

class UploadTester:
    def __init__(self, app_path: str):
        self.app_path = Path(app_path)
        self.test_dir = Path(tempfile.mkdtemp(prefix="docproc_test_"))
        self.results = []
        self.sample_files = {}

    def setup(self):
        """Create sample test files"""
        print("Setting up test files...")

        # Create sample PDF (minimal valid PDF)
        pdf_content = b"""%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >> endobj
4 0 obj << /Length 44 >> stream
BT /F1 12 Tf 100 700 Td (Test PDF Document) Tj ET
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
307
%%EOF"""

        pdf_path = self.test_dir / "test_document.pdf"
        pdf_path.write_bytes(pdf_content)
        self.sample_files['pdf'] = pdf_path

        # Create sample TXT
        txt_path = self.test_dir / "test_document.txt"
        txt_path.write_text("To jest testowy dokument.\nUmowa najmu lokalu.\nParagraf pierwszy.", encoding='utf-8')
        self.sample_files['txt'] = txt_path

        # Create sample DOCX (minimal)
        docx_path = self.test_dir / "test_document.docx"
        self._create_minimal_docx(docx_path)
        self.sample_files['docx'] = docx_path

        # Create watch folder
        self.watch_dir = self.test_dir / "watch"
        self.watch_dir.mkdir()

        print(f"Test files created in: {self.test_dir}")

    def _create_minimal_docx(self, path: Path):
        """Create minimal DOCX file"""
        import zipfile

        content_types = '''<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>'''

        rels = '''<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>'''

        document = '''<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body><w:p><w:r><w:t>Test DOCX Document - Umowa</w:t></w:r></w:p></w:body>
</w:document>'''

        with zipfile.ZipFile(path, 'w') as zf:
            zf.writestr('[Content_Types].xml', content_types)
            zf.writestr('_rels/.rels', rels)
            zf.writestr('word/document.xml', document)

    def test_backend_commands(self):
        """Test Tauri backend commands directly"""
        print("\n=== Testing Backend Commands ===")

        tests = [
            ("get_settings", "Check settings retrieval"),
            ("get_stats", "Check stats retrieval"),
            ("get_recent_documents", "Check recent documents"),
        ]

        # Note: Can't call Tauri commands directly without the app running
        # This would require integration with the running app

        for cmd, desc in tests:
            self.results.append({
                "test": f"backend_{cmd}",
                "description": desc,
                "status": "MANUAL",
                "note": "Requires running app with DevTools open"
            })

    def test_file_processing(self):
        """Test file processing via CLI or API"""
        print("\n=== Testing File Processing ===")

        for file_type, file_path in self.sample_files.items():
            print(f"  Testing {file_type.upper()}: {file_path}")

            # Check if file exists
            exists = file_path.exists()
            size = file_path.stat().st_size if exists else 0

            self.results.append({
                "test": f"file_{file_type}_exists",
                "description": f"{file_type.upper()} test file creation",
                "status": "PASS" if exists and size > 0 else "FAIL",
                "file": str(file_path),
                "size": size
            })

    def test_drag_drop_simulation(self):
        """Document drag & drop test scenarios"""
        print("\n=== Drag & Drop Test Scenarios ===")

        scenarios = [
            {
                "name": "single_pdf_drop",
                "description": "Drop single PDF file",
                "files": [self.sample_files['pdf']],
                "expected": "File should be processed"
            },
            {
                "name": "multi_file_drop",
                "description": "Drop multiple files at once",
                "files": list(self.sample_files.values()),
                "expected": "All files should be processed"
            },
            {
                "name": "invalid_file_drop",
                "description": "Drop unsupported file type",
                "files": [self.test_dir / "test.xyz"],
                "expected": "Should show error or ignore"
            },
        ]

        # Create invalid file
        (self.test_dir / "test.xyz").write_text("invalid")

        for scenario in scenarios:
            print(f"  Scenario: {scenario['name']}")
            self.results.append({
                "test": f"dragdrop_{scenario['name']}",
                "description": scenario['description'],
                "status": "MANUAL",
                "files": [str(f) for f in scenario['files']],
                "expected": scenario['expected'],
                "instructions": f"Drag these files to the app window: {[str(f) for f in scenario['files']]}"
            })

    def test_folder_picker(self):
        """Test folder picker functionality"""
        print("\n=== Folder Picker Test ===")

        self.results.append({
            "test": "folder_picker_open",
            "description": "Click 'Change Folder' button",
            "status": "MANUAL",
            "expected": "Native folder picker dialog should open",
            "instructions": "Click 'Change Folder' button and select a folder"
        })

        self.results.append({
            "test": "folder_picker_select",
            "description": "Select a folder",
            "status": "MANUAL",
            "test_folder": str(self.watch_dir),
            "expected": "Selected folder path should appear in UI",
            "instructions": f"Select this folder: {self.watch_dir}"
        })

    def test_watch_folder(self):
        """Test watch folder functionality"""
        print("\n=== Watch Folder Test ===")

        self.results.append({
            "test": "watch_folder_auto_process",
            "description": "Auto-process files added to watch folder",
            "status": "MANUAL",
            "instructions": f"1. Set watch folder to: {self.watch_dir}\n2. Copy a PDF to that folder\n3. Check if it's auto-processed"
        })

    def generate_report(self):
        """Generate test report"""
        report = {
            "title": "Document Processor Upload Test Report v2.0",
            "timestamp": datetime.now().isoformat(),
            "test_dir": str(self.test_dir),
            "sample_files": {k: str(v) for k, v in self.sample_files.items()},
            "results": self.results,
            "summary": {
                "total": len(self.results),
                "pass": len([r for r in self.results if r['status'] == 'PASS']),
                "fail": len([r for r in self.results if r['status'] == 'FAIL']),
                "manual": len([r for r in self.results if r['status'] == 'MANUAL']),
            }
        }

        # Save JSON report
        report_path = self.test_dir / "test_report.json"
        with open(report_path, 'w') as f:
            json.dump(report, f, indent=2)

        # Print summary
        print("\n" + "="*60)
        print("TEST REPORT SUMMARY")
        print("="*60)
        print(f"Total tests: {report['summary']['total']}")
        print(f"  PASS: {report['summary']['pass']}")
        print(f"  FAIL: {report['summary']['fail']}")
        print(f"  MANUAL: {report['summary']['manual']}")
        print(f"\nTest files: {self.test_dir}")
        print(f"Report: {report_path}")

        # Print manual test instructions
        print("\n" + "="*60)
        print("MANUAL TEST INSTRUCTIONS")
        print("="*60)

        for r in self.results:
            if r['status'] == 'MANUAL':
                print(f"\n[{r['test']}] {r['description']}")
                if 'instructions' in r:
                    print(f"  Instructions: {r['instructions']}")
                if 'expected' in r:
                    print(f"  Expected: {r['expected']}")

        return report

    def cleanup(self):
        """Cleanup test files (optional)"""
        # Don't cleanup - keep files for manual testing
        print(f"\nTest files kept at: {self.test_dir}")

def main():
    app_path = Path.home() / "projects/buildonai/document-processor"

    print("="*60)
    print("Document Processor Upload Tester v2.0")
    print("="*60)

    tester = UploadTester(app_path)
    tester.setup()
    tester.test_backend_commands()
    tester.test_file_processing()
    tester.test_drag_drop_simulation()
    tester.test_folder_picker()
    tester.test_watch_folder()
    report = tester.generate_report()

    print("\n" + "="*60)
    print("KNOWN ISSUES TO FIX:")
    print("="*60)
    print("""
1. DRAG & DROP:
   - Problem: file.path is undefined in Tauri webview
   - Fix: Use Tauri's drag-drop event listener instead of DOM events
   - Code: listen('tauri://file-drop', ...)

2. FOLDER PICKER:
   - Problem: Plugin may not be initialized
   - Fix: Check tauri.conf.json plugins section
   - Check: Console for errors when clicking button

3. RECOMMENDED FIXES:
   - Add error display in UI
   - Add console.log for debugging
   - Test with DevTools open (F12)
""")

    return 0

if __name__ == "__main__":
    sys.exit(main())
