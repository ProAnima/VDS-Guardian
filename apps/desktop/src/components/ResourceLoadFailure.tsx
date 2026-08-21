import { CircleAlert, LoaderCircle, RefreshCw } from "lucide-react";

interface ResourceLoadFailureProps {
  message: string;
  onRetry: () => void;
  retryLabel: string;
  retrying: boolean;
}

export function ResourceLoadFailure({ message, onRetry, retryLabel, retrying }: ResourceLoadFailureProps) {
  return <div className="resource-load-failure" role="alert">
    <CircleAlert size={18} />
    <p>{message}</p>
    <button className="button button--secondary" disabled={retrying} onClick={onRetry} type="button">
      {retrying ? <LoaderCircle className="spin" size={15} /> : <RefreshCw size={15} />}{retryLabel}
    </button>
  </div>;
}
