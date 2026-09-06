/** JSON transport with host-owned activity hooks. No DOM or page state. */
export function createJsonClient({ fetchImpl = fetch, begin = () => {}, end = () => {} } = {}) {
  return {
    async getJson(url) {
      begin();
      try {
        const res = await fetchImpl(url, { headers: { accept: "application/json" } });
        if (!res.ok) throw new Error("HTTP " + res.status);
        return await res.json();
      } finally {
        end();
      }
    },
    async postJson(url, body, opts) {
      begin();
      try {
        const res = await fetchImpl(url, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body),
          ...(opts || {}),
        });
        let data = null;
        try { data = await res.json(); } catch { data = null; }
        if (!res.ok || (data && data.ok === false)) {
          throw new Error((data && data.error) || "HTTP " + res.status);
        }
        return data;
      } finally {
        end();
      }
    },
  };
}
