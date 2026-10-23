import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openExternal } from "../lib/bridge";
export function Markdown({
  text,
  onTimestamp,
  onSource,
}: {
  text: string;
  onTimestamp?: (seconds: number) => void;
  onSource?: (url: string) => void;
}) {
  const prepared = text.replace(
    /\[(\d{1,3}:\d{2}(?::\d{2})?)\](?!\()/g,
    "[$1](#t=$1)",
  );
  return (
    <div className="markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        urlTransform={(url) =>
          url.startsWith("tuck://meeting/")
            ? url
            : /^(https?:|#)/.test(url)
              ? url
              : ""
        }
        components={{
          img: ({ alt }) => (
            <span className="markdown-image-note">
              {alt ? `Image: ${alt}` : "Image omitted"}
            </span>
          ),
          a: ({ href, children }) => {
            if (href?.startsWith("tuck://meeting/"))
              return (
                <button
                  className="citation"
                  disabled={!onSource}
                  onClick={() => onSource?.(href)}
                  title="Open meeting source"
                >
                  {children}
                </button>
              );
            if (href?.startsWith("#t=")) {
              const time = href
                .slice(3)
                .split(":")
                .reduce((s, n) => s * 60 + Number(n), 0);
              return (
                <button
                  className="citation"
                  onClick={() => onTimestamp?.(time)}
                  title="Show this moment in the transcript"
                >
                  {children}
                </button>
              );
            }
            return (
              <a
                href={href}
                onClick={(e) => {
                  e.preventDefault();
                  if (href) void openExternal(href);
                }}
              >
                {children}
              </a>
            );
          },
        }}
      >
        {prepared}
      </ReactMarkdown>
    </div>
  );
}
