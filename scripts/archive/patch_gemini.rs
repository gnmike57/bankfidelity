use crate::engine::model::Transaction;
use crate::ai::gemini_client::GeminiError;
use rust_decimal::Decimal;

pub async fn repair_extracted_transactions(
    &self,
    transactions: &[Transaction],
    opening_balance: Decimal,
    closing_balance: Decimal,
    raw_ocr_text: &str,
    error_message: &str,
) -> Result<Vec<Transaction>, GeminiError> {
    let schema = serde_json::json!({
        "type": "ARRAY",
        "items": {
            "type": "OBJECT",
            "properties": {
                "date": { "type": "STRING", "description": "Transaction date, e.g., '10/24'" },
                "description": { "type": "STRING" },
                "debit": { "type": "NUMBER", "nullable": true },
                "credit": { "type": "NUMBER", "nullable": true },
                "running_balance": { "type": "NUMBER", "nullable": true }
            },
            "required": ["date", "description"]
        }
    });

    let scrubbed = crate::ai::gemini_client::scrub_pii(transactions);
    
    let prompt = format!(
        "You are an expert financial data repair AI. The OCR extraction failed the math verification test.\n\
         Opening Balance: ${}\n\
         Target Closing Balance: ${}\n\n\
         Math Error: {}\n\n\
         Current Extracted Transactions:\n{}\n\n\
         Raw OCR Text:\n{}\n\n\
         Fix the transactions based on the raw OCR text. The math MUST sum perfectly. Return the full repaired list.",
         opening_balance, closing_balance, error_message, scrubbed, raw_ocr_text
    );

    let body = serde_json::json!({
        "contents": [{
            "role": "user",
            "parts": [{"text": prompt}]
        }],
        "generationConfig": {
            "temperature": 0.0,
            "responseMimeType": "application/json",
            "responseSchema": schema
        }
    });

    let response = self.post_generate_pro(&body).await?;

    if !response.status().is_success() {
        return Err(GeminiError::Api(response.status(), response.text().await.unwrap_or_default()));
    }

    let json_resp: serde_json::Value = response.json().await?;
    let text = json_resp["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or_else(|| GeminiError::InvalidResponse("Missing text field".into()))?;

    let repaired: Vec<Transaction> = serde_json::from_str(text)
        .map_err(|e| GeminiError::InvalidResponse(format!("JSON parse error: {e}")))?;

    Ok(repaired)
}
