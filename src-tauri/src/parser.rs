use chrono::Utc;
use image::GenericImageView;
use lopdf::Document as PdfDocument;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedDocument {
    pub id: String,
    pub filename: String,
    pub original_path: String,
    pub doc_type: Option<String>,
    pub classification_confidence: Option<f64>,
    pub pages: Option<u32>,
    pub word_count: Option<u32>,
    pub size: u64,
    pub full_text: Option<String>,
    pub text_preview: Option<String>,
    pub metadata: HashMap<String, String>,
    pub processed_at: String,
    pub images: Vec<ExtractedImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedImage {
    pub id: String,
    pub filename: String,
    pub page: Option<u32>,
    pub position_marker: Option<String>,
    pub context_before: Option<String>,
    pub context_after: Option<String>,
    pub ocr_text: Option<String>,
    pub ai_description: Option<String>,
    pub image_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

pub struct DocumentProcessor {
    output_dir: PathBuf,
}

impl DocumentProcessor {
    pub fn new(data_dir: PathBuf) -> Self {
        let output_dir = data_dir.join("przetworzone");
        fs::create_dir_all(&output_dir).ok();
        fs::create_dir_all(data_dir.join("download")).ok();
        Self { output_dir }
    }

    pub async fn process(&self, path: &Path) -> Result<ProcessedDocument, Box<dyn std::error::Error + Send + Sync>> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        match extension.as_str() {
            "pdf" => self.process_pdf(path).await,
            "docx" | "doc" => self.process_docx(path).await,
            "txt" | "text" => self.process_txt(path).await,
            _ => Err(format!("Unsupported file format: {}", extension).into()),
        }
    }

    async fn process_pdf(&self, path: &Path) -> Result<ProcessedDocument, Box<dyn std::error::Error + Send + Sync>> {
        let id = Uuid::new_v4().to_string();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let file_size = fs::metadata(path)?.len();

        // Create output directory for this document
        let doc_output_dir = self.output_dir.join(&id);
        let images_dir = doc_output_dir.join("images");
        fs::create_dir_all(&images_dir)?;

        // Extract text using pdf-extract
        let text = pdf_extract::extract_text(path).unwrap_or_default();

        // Parse PDF structure with lopdf for images
        let pdf_doc = PdfDocument::load(path)?;
        let pages = pdf_doc.get_pages().len() as u32;

        // Extract images with context
        let images = self.extract_pdf_images(&pdf_doc, &text, &images_dir)?;

        // Calculate word count
        let word_count = text.split_whitespace().count() as u32;

        // Create text preview
        let text_preview = text.chars().take(500).collect::<String>();

        // Classify document type based on content
        let (doc_type, confidence) = self.classify_document(&text);

        // Extract metadata
        let mut metadata = HashMap::new();
        if let Ok(info) = pdf_doc.trailer.get(b"Info") {
            if let Ok(info_ref) = info.as_reference() {
                if let Ok(info_dict) = pdf_doc.get_dictionary(info_ref) {
                    for (key, value) in info_dict.iter() {
                        if let Ok(value_str) = value.as_string() {
                            let key_str = String::from_utf8_lossy(key).to_string();
                            metadata.insert(key_str, value_str.into_owned());
                        }
                    }
                }
            }
        }

        // Save markdown output
        self.save_markdown(&doc_output_dir, &filename, &text, &images, &metadata)?;

        // Save JSON output
        let doc = ProcessedDocument {
            id,
            filename,
            original_path: path.to_string_lossy().to_string(),
            doc_type: Some(doc_type),
            classification_confidence: Some(confidence),
            pages: Some(pages),
            word_count: Some(word_count),
            size: file_size,
            full_text: Some(text),
            text_preview: Some(text_preview),
            metadata,
            processed_at: Utc::now().to_rfc3339(),
            images,
        };

        self.save_json(&doc_output_dir, &doc)?;

        // Copy original
        fs::copy(path, doc_output_dir.join("original.pdf")).ok();

        Ok(doc)
    }

    fn extract_pdf_images(
        &self,
        pdf_doc: &PdfDocument,
        full_text: &str,
        images_dir: &Path,
    ) -> Result<Vec<ExtractedImage>, Box<dyn std::error::Error + Send + Sync>> {
        let mut images = Vec::new();
        let text_chars: Vec<char> = full_text.chars().collect();
        let num_pages = pdf_doc.get_pages().len();

        // Iterate through all objects looking for images
        for (object_id, object) in pdf_doc.objects.iter() {
            if let Ok(stream) = object.as_stream() {
                // Check if this is an image
                if let Ok(subtype) = stream.dict.get(b"Subtype") {
                    if subtype.as_name().map(|n| n == b"Image").unwrap_or(false) {
                        let img_id = Uuid::new_v4().to_string();
                        let img_filename = format!("img_{:03}.png", images.len() + 1);
                        let img_path = images_dir.join(&img_filename);

                        // Try to extract image data
                        if let Ok(data) = stream.decompressed_content() {
                            // Get dimensions
                            let width = stream.dict.get(b"Width")
                                .ok()
                                .and_then(|w| w.as_i64().ok())
                                .unwrap_or(0) as u32;
                            let height = stream.dict.get(b"Height")
                                .ok()
                                .and_then(|h| h.as_i64().ok())
                                .unwrap_or(0) as u32;

                            // Estimate position in text based on object order
                            let approx_pos = (images.len() * text_chars.len()) / (num_pages.max(1) * 2);

                            let context_before: String = if approx_pos > 200 && approx_pos < text_chars.len() {
                                text_chars[approx_pos.saturating_sub(200)..approx_pos].iter().collect()
                            } else {
                                String::new()
                            };

                            let context_after: String = if approx_pos + 200 < text_chars.len() {
                                text_chars[approx_pos..approx_pos + 200].iter().collect()
                            } else if approx_pos < text_chars.len() {
                                text_chars[approx_pos..].iter().collect()
                            } else {
                                String::new()
                            };

                            // Try to save image data
                            if fs::write(&img_path, &data).is_ok() {
                                // Create thumbnail if image is valid
                                let thumb_path = images_dir.join(format!("thumb_{}", img_filename));
                                if let Ok(img) = image::open(&img_path) {
                                    let thumb = img.thumbnail(200, 200);
                                    thumb.save(&thumb_path).ok();
                                }

                                images.push(ExtractedImage {
                                    id: img_id,
                                    filename: img_filename,
                                    page: None,
                                    position_marker: Some(format!("obj_{}", object_id.0)),
                                    context_before: Some(context_before),
                                    context_after: Some(context_after),
                                    ocr_text: None,
                                    ai_description: None,
                                    image_path: Some(img_path.to_string_lossy().to_string()),
                                    thumbnail_path: Some(thumb_path.to_string_lossy().to_string()),
                                    width: Some(width),
                                    height: Some(height),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(images)
    }

    async fn process_docx(&self, path: &Path) -> Result<ProcessedDocument, Box<dyn std::error::Error + Send + Sync>> {
        let id = Uuid::new_v4().to_string();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let file_size = fs::metadata(path)?.len();

        let doc_output_dir = self.output_dir.join(&id);
        let images_dir = doc_output_dir.join("images");
        fs::create_dir_all(&images_dir)?;

        // Read DOCX file
        let file = File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        // Extract text from document.xml
        let mut text = String::new();
        if let Ok(mut doc_xml) = archive.by_name("word/document.xml") {
            let mut content = String::new();
            doc_xml.read_to_string(&mut content)?;

            // Simple XML text extraction (strips tags)
            let mut in_tag = false;
            for c in content.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' => {
                        in_tag = false;
                        if text.chars().last().map(|c| !c.is_whitespace()).unwrap_or(true) {
                            text.push(' ');
                        }
                    }
                    _ if !in_tag => text.push(c),
                    _ => {}
                }
            }
        }

        // Extract images from word/media/
        let mut images = Vec::new();
        let text_chars: Vec<char> = text.chars().collect();

        for i in 0..archive.len() {
            if let Ok(mut file) = archive.by_index(i) {
                let name = file.name().to_string();
                if name.starts_with("word/media/") {
                    let img_id = Uuid::new_v4().to_string();
                    let img_filename = Path::new(&name).file_name().unwrap().to_string_lossy().to_string();
                    let img_path = images_dir.join(&img_filename);

                    let mut data = Vec::new();
                    file.read_to_end(&mut data)?;
                    fs::write(&img_path, &data)?;

                    // Get image dimensions
                    let (width, height) = if let Ok(img) = image::open(&img_path) {
                        let dims = img.dimensions();
                        // Create thumbnail
                        let thumb_path = images_dir.join(format!("thumb_{}", img_filename));
                        let thumb = img.thumbnail(200, 200);
                        thumb.save(&thumb_path).ok();
                        (Some(dims.0), Some(dims.1))
                    } else {
                        (None, None)
                    };

                    // Approximate context (simplified)
                    let approx_pos = (images.len() * text_chars.len()) / 10;
                    let context_before: String = if approx_pos > 200 && approx_pos < text_chars.len() {
                        text_chars[approx_pos.saturating_sub(200)..approx_pos].iter().collect()
                    } else {
                        String::new()
                    };

                    images.push(ExtractedImage {
                        id: img_id,
                        filename: img_filename.clone(),
                        page: None,
                        position_marker: Some(format!("media_{}", images.len())),
                        context_before: Some(context_before),
                        context_after: None,
                        ocr_text: None,
                        ai_description: None,
                        image_path: Some(img_path.to_string_lossy().to_string()),
                        thumbnail_path: Some(images_dir.join(format!("thumb_{}", img_filename)).to_string_lossy().to_string()),
                        width,
                        height,
                    });
                }
            }
        }

        let word_count = text.split_whitespace().count() as u32;
        let text_preview = text.chars().take(500).collect::<String>();
        let (doc_type, confidence) = self.classify_document(&text);

        let doc = ProcessedDocument {
            id,
            filename,
            original_path: path.to_string_lossy().to_string(),
            doc_type: Some(doc_type),
            classification_confidence: Some(confidence),
            pages: None,
            word_count: Some(word_count),
            size: file_size,
            full_text: Some(text.clone()),
            text_preview: Some(text_preview),
            metadata: HashMap::new(),
            processed_at: Utc::now().to_rfc3339(),
            images,
        };

        self.save_markdown(&doc_output_dir, &doc.filename, &text, &doc.images, &doc.metadata)?;
        self.save_json(&doc_output_dir, &doc)?;
        fs::copy(path, doc_output_dir.join("original.docx")).ok();

        Ok(doc)
    }

    async fn process_txt(&self, path: &Path) -> Result<ProcessedDocument, Box<dyn std::error::Error + Send + Sync>> {
        let id = Uuid::new_v4().to_string();
        let filename = path.file_name().unwrap().to_string_lossy().to_string();
        let file_size = fs::metadata(path)?.len();

        let doc_output_dir = self.output_dir.join(&id);
        fs::create_dir_all(&doc_output_dir)?;

        // Read file with encoding detection
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        // Try UTF-8 first, then CP1250 (Polish)
        let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| {
            let (decoded, _, _) = encoding_rs::WINDOWS_1250.decode(&bytes);
            decoded.into_owned()
        });

        let word_count = text.split_whitespace().count() as u32;
        let text_preview = text.chars().take(500).collect::<String>();
        let (doc_type, confidence) = self.classify_document(&text);

        let doc = ProcessedDocument {
            id,
            filename,
            original_path: path.to_string_lossy().to_string(),
            doc_type: Some(doc_type),
            classification_confidence: Some(confidence),
            pages: None,
            word_count: Some(word_count),
            size: file_size,
            full_text: Some(text.clone()),
            text_preview: Some(text_preview),
            metadata: HashMap::new(),
            processed_at: Utc::now().to_rfc3339(),
            images: vec![],
        };

        self.save_markdown(&doc_output_dir, &doc.filename, &text, &[], &doc.metadata)?;
        self.save_json(&doc_output_dir, &doc)?;
        fs::copy(path, doc_output_dir.join("original.txt")).ok();

        Ok(doc)
    }

    fn classify_document(&self, text: &str) -> (String, f64) {
        let text_lower = text.to_lowercase();
        let _first_100 = text_lower.chars().take(100).collect::<String>();
        let first_500 = text_lower.chars().take(500).collect::<String>();

        // FIRST: Check for definitive title keywords in the header area (first 500 chars)
        // These are document type names that when appearing as title, definitively classify the doc
        // Polish documents often start with date/address, then have the title
        let title_keywords = [
            ("umowa", "umowa"),
            ("faktura", "faktura"),
            ("regulamin", "regulamin"),
            ("wniosek", "wniosek"),
            ("pozew", "pozew"),
            ("ustawa", "ustawa"),
            ("rozporządzenie", "rozporządzenie"),
            ("protokół", "protokół"),
            ("pełnomocnictwo", "pełnomocnictwo"),
            ("oświadczenie", "oświadczenie"),
            ("uchwała", "uchwała"),
            ("wyrok", "wyrok"),
            ("decyzja", "decyzja"),
            ("aneks", "aneks"),
            ("rachunek", "rachunek"),
            ("statut", "statut"),
            ("sprawozdanie", "sprawozdanie"),
            ("wezwanie", "wezwanie"),
            ("nota", "nota księgowa"),
            ("cv", "cv"),
            ("życiorys", "cv"),
            ("curriculum vitae", "cv"),
            ("pismo", "pismo"),
            ("ogólne warunki ubezpieczenia", "owu"),
            ("ogólne warunki", "owu"),
            ("owu", "owu"),
            ("polisa", "polisa"),
        ];

        // Check if document title clearly states the type (in first 500 chars)
        // Find the keyword that appears EARLIEST - that's likely the document title
        let mut earliest_match: Option<(&str, usize)> = None;
        for (keyword, doc_type) in title_keywords {
            if let Some(pos) = first_500.find(keyword) {
                if earliest_match.is_none() || pos < earliest_match.unwrap().1 {
                    earliest_match = Some((doc_type, pos));
                }
            }
        }

        if let Some((doc_type, _pos)) = earliest_match {
            return (doc_type.to_string(), 0.9);
        }

        // If no clear title, use weighted keyword matching
        // Weighted patterns: (keyword, weight) - higher weight = stronger indicator
        // Weight 3 = definitive, Weight 2 = strong, Weight 1 = weak indicator
        let patterns: Vec<(&str, Vec<(&str, u32)>)> = vec![
            // UMOWY (contracts)
            ("umowa", vec![
                // Typy umów - bardzo silne
                ("umowa najmu", 4), ("umowa o pracę", 4), ("umowa zlecenie", 4), ("umowa o dzieło", 4),
                ("umowa sprzedaży", 4), ("umowa kupna", 4), ("umowa darowizny", 4), ("umowa pożyczki", 4),
                ("umowa współpracy", 4), ("umowa powierzenia", 4), ("umowa licencyjna", 4),
                // Samo słowo umowa w nagłówku
                ("umowa", 3), ("umowa nr", 4), ("numer umowy", 3),
                // Struktura umowy
                ("strony umowy", 3), ("przedmiot umowy", 3), ("§", 2), ("postanowienia ogólne", 2),
                ("postanowienia końcowe", 2), ("zawarta w dniu", 3), ("zawarta dnia", 3),
                ("zwana dalej", 2), ("zwanym dalej", 2), ("reprezentowany przez", 2),
                // Strony
                ("wynajmujący", 2), ("najemca", 2), ("zleceniodawca", 2), ("zleceniobiorca", 2),
                ("wykonawca", 2), ("zamawiający", 2), ("usługodawca", 2), ("usługobiorca", 2),
                ("kupujący", 2), ("sprzedający", 2), ("darczyńca", 2), ("obdarowany", 2),
                // Klauzule
                ("czas trwania umowy", 2), ("wypowiedzenie umowy", 2), ("rozwiązanie umowy", 2),
                ("warunki umowy", 2), ("zobowiązuje się", 2), ("pomiędzy", 1),
                ("na mocy niniejszej umowy", 3), ("w ramach umowy", 2),
            ]),

            // ANEKS (amendment)
            ("aneks", vec![
                ("aneks do umowy", 3), ("aneks nr", 3), ("zmienia się", 2),
                ("w brzmieniu", 2), ("otrzymuje brzmienie", 2), ("dotychczasowe brzmienie", 2),
                ("niniejszym aneksem", 2), ("wprowadza się zmiany", 2), ("pozostałe postanowienia", 1),
            ]),

            // POZEW (lawsuit)
            ("pozew", vec![
                ("pozew o zapłatę", 3), ("pozew o rozwód", 3), ("pozew o odszkodowanie", 3),
                ("powód", 2), ("pozwany", 2), ("wnoszę o", 2), ("roszczenie", 2),
                ("sąd rejonowy", 2), ("sąd okręgowy", 2), ("wartość przedmiotu sporu", 3),
                ("uzasadnienie", 1), ("dowody", 1), ("załączniki", 1), ("sygnatura akt", 2),
                ("zasądzenie", 2), ("na rzecz powoda", 2), ("solidarnie", 1),
            ]),

            // ODPOWIEDŹ NA POZEW
            ("odpowiedź na pozew", vec![
                ("odpowiedź na pozew", 3), ("pozwany wnosi", 3), ("oddalenie powództwa", 3),
                ("w odpowiedzi na pozew", 3), ("zarzuty", 2), ("bezzasadne", 2),
                ("wnoszę o oddalenie", 3), ("kwestionuję", 2), ("nie zgadzam się", 1),
            ]),

            // USTAWA (act/law)
            ("ustawa", vec![
                ("ustawa z dnia", 3), ("dz.u.", 3), ("dziennik ustaw", 3),
                ("art.", 2), ("ust.", 2), ("pkt", 1), ("rozdział", 2), ("przepisy ogólne", 2),
                ("przepisy końcowe", 2), ("wchodzi w życie", 2), ("sejm rzeczypospolitej", 3),
                ("uchwala", 2), ("tekst jednolity", 2), ("nowelizacja", 2),
            ]),

            // ROZPORZĄDZENIE (regulation)
            ("rozporządzenie", vec![
                ("rozporządzenie ministra", 3), ("rozporządzenie rady ministrów", 3),
                ("na podstawie art", 2), ("zarządza się", 2), ("minister właściwy", 2),
                ("rozporządzenie wchodzi w życie", 2), ("traci moc", 2),
            ]),

            // FAKTURA (invoice)
            ("faktura", vec![
                // Nagłówki - bardzo silne
                ("faktura vat", 5), ("faktura nr", 5), ("faktura proforma", 4), ("faktura korygująca", 4),
                ("faktura", 4), ("fv/", 4), ("fvat", 4),
                // NIP - kluczowe dla faktur
                ("nip:", 3), ("nip sprzedawcy", 4), ("nip nabywcy", 4), ("nip ", 2),
                // Wartości
                ("netto", 3), ("brutto", 3), ("vat", 3), ("stawka vat", 3),
                ("wartość netto", 3), ("wartość brutto", 3), ("kwota vat", 3),
                ("razem do zapłaty", 4), ("do zapłaty", 3), ("suma", 2),
                // Płatność
                ("termin płatności", 3), ("forma płatności", 2), ("nr konta", 2), ("numer konta", 2),
                ("przelew", 1), ("gotówka", 1), ("zapłacono", 2),
                // Strony
                ("sprzedawca", 2), ("nabywca", 2), ("odbiorca", 2), ("płatnik", 2),
                // Daty i pozycje
                ("data sprzedaży", 3), ("data wystawienia", 3), ("data dostawy", 2),
                ("nazwa towaru", 2), ("jednostka miary", 2), ("ilość", 1), ("cena", 1), ("j.m.", 2),
                ("lp.", 1), ("poz.", 1),
            ]),

            // RACHUNEK (receipt/bill)
            ("rachunek", vec![
                ("rachunek nr", 3), ("rachunek za", 2), ("do rachunku", 2),
                ("wystawca rachunku", 2), ("odbiorca", 1), ("kwota", 1),
            ]),

            // PROTOKÓŁ (minutes/protocol)
            ("protokół", vec![
                ("protokół z", 3), ("protokół posiedzenia", 3), ("protokół zebrania", 3),
                ("protokół odbioru", 3), ("protokół zdawczo-odbiorczy", 3),
                ("w dniu", 1), ("obecni", 2), ("porządek obrad", 2), ("uchwała", 2),
                ("głosowanie", 2), ("protokołował", 2), ("przewodniczący", 1),
            ]),

            // WNIOSEK (application/motion)
            ("wniosek", vec![
                ("wniosek o", 3), ("wnoszę o", 2), ("proszę o", 1), ("zwracam się", 1),
                ("wniosek o wydanie", 3), ("wniosek o przyznanie", 3), ("wniosek o wpis", 3),
                ("wnioskodawca", 2), ("uzasadnienie wniosku", 2), ("podstawa prawna", 2),
            ]),

            // PEŁNOMOCNICTWO (power of attorney)
            ("pełnomocnictwo", vec![
                ("pełnomocnictwo", 3), ("upoważniam", 2), ("mocodawca", 3), ("pełnomocnik", 2),
                ("udziela pełnomocnictwa", 3), ("do reprezentowania", 2), ("w zakresie", 1),
                ("pełnomocnictwo ogólne", 3), ("pełnomocnictwo szczególne", 3),
                ("niniejszym upoważniam", 3), ("pesel", 1), ("dowód osobisty", 1),
            ]),

            // WYROK (judgment)
            ("wyrok", vec![
                ("wyrok", 2), ("w imieniu rzeczypospolitej polskiej", 3), ("sąd orzeka", 3),
                ("zasądza", 2), ("oddala", 2), ("powództwo", 2), ("apelację", 2),
                ("sentencja", 2), ("uzasadnienie wyroku", 3), ("koszty procesu", 2),
            ]),

            // POSTANOWIENIE (decision/order)
            ("postanowienie", vec![
                ("postanowienie", 2), ("postanawia", 2), ("sąd postanawia", 3),
                ("zażalenie", 2), ("zarządzenie", 2),
            ]),

            // DECYZJA (administrative decision)
            ("decyzja", vec![
                ("decyzja nr", 3), ("decyzja administracyjna", 3), ("organ wydający", 2),
                ("na podstawie art", 2), ("orzeka", 2), ("postanawia", 1),
                ("odwołanie", 2), ("pouczenie", 2), ("strona decyzji", 2),
            ]),

            // OŚWIADCZENIE (declaration/statement)
            ("oświadczenie", vec![
                ("oświadczenie", 2), ("oświadczam", 3), ("niniejszym oświadczam", 3),
                ("pod rygorem odpowiedzialności", 2), ("świadomy odpowiedzialności", 2),
                ("oświadczenie o", 2), ("zgodne z prawdą", 2),
            ]),

            // UCHWAŁA (resolution)
            ("uchwała", vec![
                ("uchwała nr", 3), ("uchwała zarządu", 3), ("uchwała rady", 3),
                ("uchwała wspólników", 3), ("uchwała walnego zgromadzenia", 3),
                ("postanawia", 1), ("uchwala się", 2), ("w głosowaniu", 2),
            ]),

            // SPRAWOZDANIE (report)
            ("sprawozdanie", vec![
                ("sprawozdanie z", 3), ("sprawozdanie finansowe", 3), ("sprawozdanie roczne", 3),
                ("bilans", 2), ("rachunek zysków i strat", 2), ("zarząd spółki", 1),
            ]),

            // REGULAMIN (regulations/rules)
            ("regulamin", vec![
                ("regulamin", 2), ("regulamin pracy", 3), ("regulamin organizacyjny", 3),
                ("postanowienia ogólne", 1), ("zakres obowiązywania", 2), ("przepisy końcowe", 1),
            ]),

            // STATUT (statute/charter)
            ("statut", vec![
                ("statut", 2), ("statut spółki", 3), ("statut stowarzyszenia", 3),
                ("statut fundacji", 3), ("cele statutowe", 2), ("zmiana statutu", 2),
            ]),

            // LIST (letter)
            ("pismo", vec![
                ("szanowni państwo", 2), ("szanowny panie", 2), ("szanowna pani", 2),
                ("w odpowiedzi na", 2), ("w nawiązaniu do", 2), ("z poważaniem", 2),
                ("uprzejmie informuję", 2), ("uprzejmie proszę", 2),
            ]),

            // NOTA KSIĘGOWA (accounting note)
            ("nota księgowa", vec![
                ("nota księgowa", 3), ("nota obciążeniowa", 3), ("nota uznaniowa", 3),
                ("korekta", 2), ("obciążenie", 1), ("uznanie", 1),
            ]),

            // WEZWANIE (summons/demand)
            ("wezwanie", vec![
                ("wezwanie do zapłaty", 3), ("wezwanie przedsądowe", 3),
                ("wzywam do", 2), ("ostateczne wezwanie", 3), ("pod rygorem", 2),
                ("w terminie", 1), ("zapłaty kwoty", 2),
            ]),

            // CV / ŻYCIORYS
            ("cv", vec![
                ("curriculum vitae", 3), ("życiorys", 2), ("doświadczenie zawodowe", 3),
                ("wykształcenie", 2), ("umiejętności", 2), ("znajomość języków", 2),
                ("stanowisko", 1), ("prawo jazdy", 1), ("hobby", 1),
            ]),

            // LIST MOTYWACYJNY
            ("list motywacyjny", vec![
                ("list motywacyjny", 3), ("aplikuję na stanowisko", 3),
                ("z zainteresowaniem", 2), ("moje doświadczenie", 2), ("pragnę", 1),
            ]),
        ];

        let mut best_match = ("unknown".to_string(), 0.0);
        let mut best_score = 0u32;

        for (doc_type, keywords) in patterns {
            let mut score = 0u32;
            let mut max_possible = 0u32;

            for (keyword, weight) in &keywords {
                max_possible += weight;

                // Check title/header area first (stronger indicator)
                if first_500.contains(keyword) {
                    score += weight * 2; // Double weight for header matches
                } else if text_lower.contains(keyword) {
                    score += weight;
                }
            }

            // Calculate confidence as weighted ratio
            let confidence = if max_possible > 0 {
                (score as f64) / (max_possible as f64 * 2.0) // *2 because max would be all in header
            } else {
                0.0
            };

            if score > best_score {
                best_score = score;
                best_match = (doc_type.to_string(), confidence.min(0.99));
            }
        }

        // Minimum threshold - if score is too low, mark as unknown
        // Score 2 = at least one weak keyword match in header, or strong keyword anywhere
        if best_score < 2 {
            return ("unknown".to_string(), 0.0);
        }

        best_match
    }

    fn save_markdown(
        &self,
        output_dir: &Path,
        filename: &str,
        text: &str,
        images: &[ExtractedImage],
        metadata: &HashMap<String, String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut md = String::new();

        md.push_str(&format!("# Document: {}\n\n", filename));
        md.push_str(&format!("**Processed:** {}\n\n", Utc::now().format("%Y-%m-%d %H:%M:%S")));

        if !metadata.is_empty() {
            md.push_str("## Metadata\n\n");
            for (key, value) in metadata {
                md.push_str(&format!("- **{}:** {}\n", key, value));
            }
            md.push_str("\n");
        }

        md.push_str("---\n\n## Content\n\n");

        // Insert images at approximate positions
        let text_chars: Vec<char> = text.chars().collect();
        let mut last_pos = 0;

        for (i, img) in images.iter().enumerate() {
            // Calculate where to insert image reference
            let insert_pos = if let Some(ref marker) = img.position_marker {
                if marker.contains("page_") {
                    // For PDF: insert after each ~1000 chars
                    ((i + 1) * 1000).min(text_chars.len())
                } else {
                    ((i + 1) * text_chars.len() / (images.len() + 1)).min(text_chars.len())
                }
            } else {
                text_chars.len()
            };

            // Add text before image
            if insert_pos > last_pos {
                let segment: String = text_chars[last_pos..insert_pos].iter().collect();
                md.push_str(&segment);
                md.push_str("\n\n");
            }

            // Add image reference
            md.push_str(&format!("![Image {}](images/{})\n", i + 1, img.filename));
            if let Some(ref ctx) = img.context_before {
                md.push_str(&format!("*Context: ...{}*\n", ctx.chars().take(100).collect::<String>()));
            }
            if let Some(ref desc) = img.ai_description {
                md.push_str(&format!("*AI Description: {}*\n", desc));
            }
            md.push_str("\n");

            last_pos = insert_pos;
        }

        // Add remaining text
        if last_pos < text_chars.len() {
            let segment: String = text_chars[last_pos..].iter().collect();
            md.push_str(&segment);
        }

        fs::write(output_dir.join("document.md"), md)?;

        Ok(())
    }

    fn save_json(
        &self,
        output_dir: &Path,
        doc: &ProcessedDocument,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let json = serde_json::to_string_pretty(doc)?;
        fs::write(output_dir.join("document.json"), json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_processor() -> (DocumentProcessor, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let processor = DocumentProcessor::new(temp_dir.path().to_path_buf());
        (processor, temp_dir)
    }

    // ============ CLASSIFICATION TESTS ============

    #[test]
    fn test_classify_umowa() {
        let (processor, _temp) = create_test_processor();
        let text = "UMOWA NAJMU LOKALU
            Strony umowy:
            Wynajmujący: Jan Kowalski
            Najemca: Anna Nowak
            Przedmiot umowy: lokal mieszkalny
            § 1. Postanowienia ogólne";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "umowa");
        assert!(confidence > 0.1, "Confidence should be > 0.1, got {}", confidence);
    }

    #[test]
    fn test_classify_faktura() {
        let (processor, _temp) = create_test_processor();
        let text = "FAKTURA VAT nr 123/2024
            NIP sprzedawcy: 1234567890
            NIP nabywcy: 0987654321
            Wartość netto: 1000 zł
            Stawka VAT 23%: 230 zł
            Kwota brutto: 1230 zł
            Termin płatności: 14 dni";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "faktura");
        assert!(confidence > 0.1);
    }

    #[test]
    fn test_classify_pozew() {
        let (processor, _temp) = create_test_processor();
        let text = "POZEW O ZAPŁATĘ
            Do Sądu Rejonowego
            Powód: Jan Kowalski
            Pozwany: XYZ Sp. z o.o.
            Wartość przedmiotu sporu: 10000 zł
            Wnoszę o zasądzenie kwoty
            Roszczenie wynika z umowy";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "pozew");
        assert!(confidence > 0.1);
    }

    #[test]
    fn test_classify_ustawa() {
        let (processor, _temp) = create_test_processor();
        let text = "USTAWA z dnia 1 stycznia 2024 r.
            o zmianie niektórych ustaw
            Dz.U. 2024 poz. 123
            Art. 1. Zakres ustawy
            Rozdział 1. Przepisy ogólne
            Art. 2. Wchodzi w życie po upływie 14 dni";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "ustawa");
        assert!(confidence > 0.1);
    }

    #[test]
    fn test_classify_wniosek() {
        let (processor, _temp) = create_test_processor();
        let text = "WNIOSEK O WYDANIE ZAŚWIADCZENIA
            Wnioskodawca: Jan Kowalski
            Wnoszę o wydanie zaświadczenia
            Podstawa prawna: art. 217 KPA
            Uzasadnienie wniosku: potrzebuję do banku";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "wniosek");
        assert!(confidence > 0.1);
    }

    #[test]
    fn test_classify_pelnomocnictwo() {
        let (processor, _temp) = create_test_processor();
        let text = "PEŁNOMOCNICTWO
            Mocodawca: Jan Kowalski, PESEL 12345678901
            Niniejszym upoważniam Annę Nowak
            Pełnomocnik upoważniony jest do reprezentowania
            przed sądami i urzędami";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "pełnomocnictwo");
        assert!(confidence > 0.1);
    }

    #[test]
    fn test_classify_wyrok() {
        let (processor, _temp) = create_test_processor();
        let text = "WYROK
            W IMIENIU RZECZYPOSPOLITEJ POLSKIEJ
            Sąd Rejonowy w Warszawie
            Sąd orzeka:
            I. Zasądza od pozwanego na rzecz powoda kwotę 5000 zł
            II. Koszty procesu ponosi pozwany";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "wyrok");
        assert!(confidence > 0.1);
    }

    #[test]
    fn test_classify_unknown() {
        let (processor, _temp) = create_test_processor();
        let text = "Lorem ipsum dolor sit amet consectetur adipiscing elit";

        let (doc_type, _confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "unknown");
    }

    #[test]
    fn test_classify_wezwanie() {
        let (processor, _temp) = create_test_processor();
        let text = "WEZWANIE DO ZAPŁATY
            Ostateczne wezwanie przedsądowe
            Wzywam do zapłaty kwoty 5000 zł
            w terminie 7 dni pod rygorem skierowania sprawy na drogę sądową";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "wezwanie");
        assert!(confidence > 0.1);
    }

    #[test]
    fn test_classify_oswiadczenie() {
        let (processor, _temp) = create_test_processor();
        let text = "OŚWIADCZENIE
            Niniejszym oświadczam, że dane zawarte w formularzu są zgodne z prawdą.
            Świadomy odpowiedzialności karnej za składanie fałszywych zeznań
            oświadczam, że nie posiadam zaległości podatkowych.";

        let (doc_type, confidence) = processor.classify_document(text);
        assert_eq!(doc_type, "oświadczenie");
        assert!(confidence > 0.1);
    }

    // ============ TXT PROCESSING TESTS ============

    #[tokio::test]
    async fn test_process_txt_file() {
        let (processor, temp_dir) = create_test_processor();

        // Create test file
        let test_file = temp_dir.path().join("test_umowa.txt");
        let mut file = File::create(&test_file).unwrap();
        writeln!(file, "UMOWA NAJMU").unwrap();
        writeln!(file, "Strony umowy: Wynajmujący i Najemca").unwrap();
        writeln!(file, "Przedmiot umowy: lokal mieszkalny").unwrap();

        let result = processor.process(&test_file).await;
        assert!(result.is_ok(), "Processing should succeed");

        let doc = result.unwrap();
        assert_eq!(doc.filename, "test_umowa.txt");
        assert_eq!(doc.doc_type, Some("umowa".to_string()));
        assert!(doc.word_count.unwrap() > 0);
        assert!(doc.full_text.is_some());
    }

    #[tokio::test]
    async fn test_process_txt_word_count() {
        let (processor, temp_dir) = create_test_processor();

        let test_file = temp_dir.path().join("word_count_test.txt");
        let mut file = File::create(&test_file).unwrap();
        writeln!(file, "jeden dwa trzy cztery pięć").unwrap();

        let result = processor.process(&test_file).await.unwrap();
        assert_eq!(result.word_count, Some(5));
    }

    // ============ EDGE CASES ============

    #[tokio::test]
    async fn test_process_empty_file() {
        let (processor, temp_dir) = create_test_processor();

        let test_file = temp_dir.path().join("empty.txt");
        File::create(&test_file).unwrap();

        let result = processor.process(&test_file).await.unwrap();
        assert_eq!(result.word_count, Some(0));
    }

    #[tokio::test]
    async fn test_process_polish_characters() {
        let (processor, temp_dir) = create_test_processor();

        let test_file = temp_dir.path().join("polish.txt");
        let mut file = File::create(&test_file).unwrap();
        writeln!(file, "Żółć gęślą jaźń ĄĆĘŁŃÓŚŹŻ").unwrap();

        let result = processor.process(&test_file).await.unwrap();
        assert!(result.full_text.unwrap().contains("Żółć"));
    }

    #[tokio::test]
    async fn test_unsupported_format() {
        let (processor, temp_dir) = create_test_processor();

        let test_file = temp_dir.path().join("test.xyz");
        File::create(&test_file).unwrap();

        let result = processor.process(&test_file).await;
        assert!(result.is_err());
    }

    // ============ OUTPUT TESTS ============

    #[tokio::test]
    async fn test_creates_output_directory() {
        let (processor, temp_dir) = create_test_processor();

        let test_file = temp_dir.path().join("output_test.txt");
        let mut file = File::create(&test_file).unwrap();
        writeln!(file, "Test content").unwrap();

        let result = processor.process(&test_file).await.unwrap();

        // Check that output directory was created
        let output_dir = temp_dir.path().join("przetworzone").join(&result.id);
        assert!(output_dir.exists(), "Output directory should exist");
        assert!(output_dir.join("document.json").exists(), "JSON should exist");
        assert!(output_dir.join("document.md").exists(), "Markdown should exist");
    }

    // ============ SERIALIZATION TESTS ============

    #[test]
    fn test_processed_document_serialization() {
        let doc = ProcessedDocument {
            id: "test-id".to_string(),
            filename: "test.pdf".to_string(),
            original_path: "/path/to/test.pdf".to_string(),
            doc_type: Some("umowa".to_string()),
            classification_confidence: Some(0.85),
            pages: Some(5),
            word_count: Some(1000),
            size: 50000,
            full_text: Some("Test content".to_string()),
            text_preview: Some("Test...".to_string()),
            metadata: HashMap::new(),
            processed_at: "2024-01-01T00:00:00Z".to_string(),
            images: vec![],
        };

        let json = serde_json::to_string(&doc);
        assert!(json.is_ok());

        let parsed: Result<ProcessedDocument, _> = serde_json::from_str(&json.unwrap());
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().id, "test-id");
    }
}
