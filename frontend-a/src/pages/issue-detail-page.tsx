import { useParams } from "react-router-dom";
import { IssueDetailRoute } from "@multica/views/issues/components";
import { useWorkspaceId } from "@multica/core/hooks";
import { useCanonicalIssue } from "@multica/core/issues/canonical-id";
import { useDocumentTitle } from "../hooks/use-document-title";

export function IssueDetailPage({ onDelete }: { onDelete?: () => void }) {
  const { id } = useParams<{ id: string }>();
  const wsId = useWorkspaceId();
  // `id` may be an identifier (`MUL-123`); resolving here means the title
  // watches the same UUID-keyed entry realtime events patch.
  const { issue } = useCanonicalIssue(wsId, id ?? "");

  useDocumentTitle(issue ? `${issue.identifier}: ${issue.title}` : "Issue");

  if (!id) return null;
  return <IssueDetailRoute routeId={id} onDelete={onDelete} />;
}
