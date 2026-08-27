use crate::cma::models::{CmaApiResponse, CmaSurfaceData};
use chrono::Local;
use std::io;

const CMA_API_HOST: &str = "api.data.cma.cn:8090";
const CMA_API_BASE: &str = "/api";
const CMA_DATA_CODE_HOR: &str = "SURF_CHN_MUL_HOR";
const CMA_INTERFACE_ID: &str = "getSurfEleByTimeRangeAndStaID";
const CMA_ELEMENTS: &str =
    "Station_Id_C,Year,Mon,Day,Hour,TEM,PRS,RHU,PRE_1h,WIN_S_Avg_2mi,WIN_D_Avg_2mi,VIS";

#[derive(Clone)]
pub struct CmaClient {
    user_id: String,
    pwd: String,
}

impl CmaClient {
    pub fn new(user_id: String, pwd: String) -> Self {
        Self { user_id, pwd }
    }

    /// 查询指定站点在 time_range 内的逐小时地面观测数据
    /// time_range 格式: [YYYYMMDDHHMISS,YYYYMMDDHHMISS]
    pub async fn query_surface_data(
        &self,
        station_ids: &[String],
        time_range: &str,
    ) -> Result<Vec<CmaSurfaceData>, CmaClientError> {
        if station_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sta_ids = station_ids.join(",");

        let query = format!(
            "userId={}&pwd={}&dataFormat=json&interfaceId={}&dataCode={}&timeRange={}&staIDs={}&elements={}",
            url_encode(&self.user_id),
            url_encode(&self.pwd),
            CMA_INTERFACE_ID,
            CMA_DATA_CODE_HOR,
            url_encode(time_range),
            url_encode(&sta_ids),
            CMA_ELEMENTS
        );

        let request = format!(
            "GET {}?{} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\n\r\n",
            CMA_API_BASE,
            query,
            CMA_API_HOST
        );

        let response_body = self.send_http_request(&request).await?;
        let body = extract_http_body(&response_body)?;

        let api_resp: CmaApiResponse<CmaSurfaceData> =
            serde_json::from_str(body).map_err(|e| CmaClientError::Parse(e.to_string()))?;

        if api_resp.return_code != "0" {
            return Err(CmaClientError::Api {
                code: api_resp.return_code,
                message: api_resp.return_message,
            });
        }

        Ok(api_resp.ds)
    }

    /// 拉取最近 N 小时的实况数据（内部自动构造 time_range）
    pub async fn fetch_recent_surface_data(
        &self,
        station_ids: &[String],
        hours: i64,
    ) -> Result<Vec<CmaSurfaceData>, CmaClientError> {
        let end = Local::now();
        let start = end - chrono::Duration::hours(hours);
        let time_range = format!(
            "[{},{}]",
            start.format("%Y%m%d%H%M%S"),
            end.format("%Y%m%d%H%M%S")
        );
        self.query_surface_data(station_ids, &time_range).await
    }

    async fn send_http_request(&self, request: &str) -> Result<String, CmaClientError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;
        use tokio::time::{timeout, Duration};

        let stream = timeout(Duration::from_secs(10), TcpStream::connect(CMA_API_HOST))
            .await
            .map_err(|_| CmaClientError::Http("连接超时".to_string()))?
            .map_err(|e| CmaClientError::Http(format!("连接失败: {}", e)))?;

        let (mut reader, mut writer) = stream.into_split();

        writer
            .write_all(request.as_bytes())
            .await
            .map_err(|e| CmaClientError::Http(format!("发送请求失败: {}", e)))?;

        let mut buf = Vec::new();
        let mut temp = [0u8; 4096];

        let read_result = timeout(Duration::from_secs(30), async {
            loop {
                match reader.read(&mut temp).await {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&temp[..n]),
                    Err(e) => return Err(CmaClientError::Http(format!("读取响应失败: {}", e))),
                }
            }
            Ok(())
        })
        .await;

        match read_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(CmaClientError::Http("读取响应超时".to_string())),
        }

        String::from_utf8(buf).map_err(|e| CmaClientError::Parse(format!("无效 UTF-8: {}", e)))
    }
}

fn extract_http_body(response: &str) -> Result<&str, CmaClientError> {
    // 找到空行分隔的 body
    if let Some(idx) = response.find("\r\n\r\n") {
        Ok(&response[idx + 4..])
    } else if let Some(idx) = response.find("\n\n") {
        Ok(&response[idx + 2..])
    } else {
        Err(CmaClientError::Parse(
            "HTTP 响应格式错误: 未找到分隔空行".to_string(),
        ))
    }
}

fn url_encode(s: &str) -> String {
    // 简单的 URL encode，只处理常见特殊字符
    let mut result = String::new();
    for c in s.chars() {
        match c {
            ' ' => result.push_str("%20"),
            '&' => result.push_str("%26"),
            '=' => result.push_str("%3D"),
            '[' => result.push_str("%5B"),
            ']' => result.push_str("%5D"),
            ',' => result.push_str("%2C"),
            ':' => result.push_str("%3A"),
            '/' => result.push_str("%2F"),
            '?' => result.push_str("%3F"),
            '#' => result.push_str("%23"),
            '%' => result.push_str("%25"),
            '+' => result.push_str("%2B"),
            c => result.push(c),
        }
    }
    result
}

#[derive(Debug, Clone)]
pub enum CmaClientError {
    Http(String),
    Parse(String),
    Api { code: String, message: String },
}

impl std::fmt::Display for CmaClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CmaClientError::Http(s) => write!(f, "HTTP 错误: {}", s),
            CmaClientError::Parse(s) => write!(f, "解析错误: {}", s),
            CmaClientError::Api { code, message } => {
                write!(f, "CMA API 错误 [{}]: {}", code, message)
            }
        }
    }
}

impl std::error::Error for CmaClientError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cma_client_new() {
        let client = CmaClient::new("test_user".to_string(), "test_pwd".to_string());
        assert_eq!(client.user_id, "test_user");
        assert_eq!(client.pwd, "test_pwd");
    }

    #[test]
    fn test_cma_client_clone() {
        let client = CmaClient::new("user".to_string(), "pwd".to_string());
        let cloned = client.clone();
        assert_eq!(cloned.user_id, "user");
    }

    #[test]
    fn test_cma_client_error_display() {
        let err = CmaClientError::Http("connection refused".to_string());
        assert_eq!(format!("{}", err), "HTTP 错误: connection refused");

        let err = CmaClientError::Parse("invalid json".to_string());
        assert_eq!(format!("{}", err), "解析错误: invalid json");

        let err = CmaClientError::Api {
            code: "1001".to_string(),
            message: "参数错误".to_string(),
        };
        assert_eq!(format!("{}", err), "CMA API 错误 [1001]: 参数错误");
    }

    #[tokio::test]
    async fn test_query_surface_data_empty_station_ids() {
        let client = CmaClient::new("user".to_string(), "pwd".to_string());
        let result = client
            .query_surface_data(&[], "[20240101000000,20240101235959]")
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a=b&c"), "a%3Db%26c");
        assert_eq!(url_encode("[2024,2025]"), "%5B2024%2C2025%5D");
    }

    #[test]
    fn test_extract_http_body() {
        let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"code\":0}";
        let body = extract_http_body(response).unwrap();
        assert_eq!(body, "{\"code\":0}");
    }

    #[test]
    fn test_extract_http_body_lf_only() {
        let response = "HTTP/1.1 200 OK\nContent-Type: json\n\n{\"code\":0}";
        let body = extract_http_body(response).unwrap();
        assert_eq!(body, "{\"code\":0}");
    }

    #[test]
    fn test_extract_http_body_invalid() {
        let response = "HTTP/1.1 200 OK";
        assert!(extract_http_body(response).is_err());
    }
}
