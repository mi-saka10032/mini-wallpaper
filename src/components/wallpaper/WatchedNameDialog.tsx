import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useCollectionStore } from "@/stores/collectionStore";

interface WatchedNameDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  collectionId: number | null;
  /** 默认填充目录名；取消或关闭时保持这个默认名 */
  defaultName: string;
}

/**
 * 新增监听目录后的独立名称确认弹窗。
 * 与监听目录弹窗分开，避免两个 DialogContent 叠在同一遮罩上。
 */
const WatchedNameDialog: React.FC<WatchedNameDialogProps> = ({
  open,
  onOpenChange,
  collectionId,
  defaultName,
}) => {
  const { t } = useTranslation();
  const renameCollection = useCollectionStore((s) => s.renameCollection);
  const [value, setValue] = useState(defaultName);

  useEffect(() => {
    if (open) setValue(defaultName);
  }, [open, defaultName]);

  const confirm = async () => {
    const name = value.trim();
    if (collectionId !== null && name && name !== defaultName) {
      try {
        await renameCollection(collectionId, name);
      } catch (e) {
        console.error("[Watched] rename failed", e);
      }
    }
    // 名称没改 / 取消 / 关闭都保持后端已填好的目录名
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>{t("watched.nameTitle")}</DialogTitle>
          <DialogDescription>{t("watched.nameLabel")}</DialogDescription>
        </DialogHeader>
        <div className="py-2">
          <Input
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void confirm();
            }}
            placeholder={t("sidebar.enterName")}
            maxLength={32}
            autoFocus
          />
          <div className="mt-1.5 flex justify-end text-xs text-muted-foreground">
            <span>{value.length}/32</span>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("sidebar.cancel")}
          </Button>
          <Button onClick={() => void confirm()}>{t("sidebar.confirm")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

export default WatchedNameDialog;
