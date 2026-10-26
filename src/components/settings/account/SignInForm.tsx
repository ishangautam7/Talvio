import React, { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Smartphone } from "@/components/icons/glyphs";
import { commands, type AccountStatus, type Result } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Segmented } from "@/components/ui/Segmented";
import { GoogleLogo } from "@/components/icons/ProviderLogos";
import { looksLikeEmail } from "@/components/feedback/feedbackStore";
import {
  accountErrorKey,
  PASSWORD_MAX,
  PASSWORD_MIN,
  signInErrorKey,
} from "./account";

type Mode = "sign_in" | "create";

interface SignInFormProps {
  /** Only true once the server has said it offers Google sign-in. */
  googleAvailable: boolean;
  /** A Google sign-in is waiting on the browser, maybe since an earlier visit. */
  googlePending: boolean;
}

const Field: React.FC<
  React.InputHTMLAttributes<HTMLInputElement> & {
    id: string;
    label: string;
    hint?: string;
  }
> = ({ id, label, hint, ...input }) => (
  <div className="space-y-1.5">
    <label htmlFor={id} className="block text-[0.8125rem] font-medium text-ink">
      {label}
    </label>
    <Input
      id={id}
      className="w-full"
      aria-describedby={hint ? `${id}-hint` : undefined}
      {...input}
    />
    {hint && (
      <p id={`${id}-hint`} className="text-xs text-muted">
        {hint}
      </p>
    )}
  </div>
);

/**
 * Signed out: email and password, or Google when the server offers it. A
 * success needs nothing here: the signed-in status arrives as an event.
 *
 * While a request runs the fields are read-only rather than disabled, and the
 * submit button stays enabled: a disabled control loses focus to the page
 * behind the Settings dialog, and Tab then leaves the dialog.
 */
export const SignInForm: React.FC<SignInFormProps> = ({
  googleAvailable,
  googlePending,
}) => {
  const { t } = useTranslation();
  const ids = useId();
  const [mode, setMode] = useState<Mode>("sign_in");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState<"password" | "google" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const creating = mode === "create";
  const waiting = busy === "google" || googlePending;
  const locked = busy !== null || googlePending;
  const googleId = `${ids}-google`;

  // The Google button and the waiting row's Cancel replace each other, so
  // the one that had focus is gone: the other takes it.
  useEffect(() => {
    if (document.activeElement === document.body) {
      document.getElementById(googleId)?.focus();
    }
  }, [waiting, googleId]);

  const run = async (
    kind: "password" | "google",
    call: () => Promise<Result<AccountStatus, string>>,
  ) => {
    setBusy(kind);
    setError(null);
    try {
      const result = await call();
      if (result.status === "error") setError(signInErrorKey(result.error));
    } catch (failure) {
      console.error("Account sign-in failed:", failure);
      setError(accountErrorKey("generic"));
    } finally {
      setBusy(null);
    }
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (locked) return;
    const address = email.trim();
    if (!looksLikeEmail(address)) {
      setError(accountErrorKey("invalid_email"));
      return;
    }
    // Code points, as the server counts; `.length` counts UTF-16 units.
    const length = Array.from(password).length;
    if (creating && (length < PASSWORD_MIN || length > PASSWORD_MAX)) {
      setError(accountErrorKey("weak_password"));
      return;
    }
    void run("password", () =>
      creating
        ? commands.accountSignUp(address, password, name.trim() || null)
        : commands.accountSignIn(address, password),
    );
  };

  return (
    <section className="rounded-2xl border border-hairline bg-surface px-6 py-8 elev-card">
      <div className="mx-auto max-w-sm">
        <div className="text-center">
          <div className="mx-auto grid h-10 w-10 place-items-center rounded-xl bg-surface-strong text-ink">
            <Smartphone className="h-5 w-5" aria-hidden="true" />
          </div>
          <h2 className="mt-3 font-display text-lg leading-tight text-ink">
            {t("settings.account.signedOut.title")}
          </h2>
          <p className="mt-1.5 text-sm leading-relaxed text-muted text-pretty">
            {t("settings.account.signedOut.body")}
          </p>
        </div>

        <Segmented
          className="mt-6"
          fill
          label={t("settings.account.signedOut.modeLabel")}
          value={mode}
          onChange={(next) => {
            setMode(next);
            setError(null);
          }}
          disabled={locked}
          options={[
            { value: "sign_in", label: t("settings.account.signedOut.signIn") },
            {
              value: "create",
              label: t("settings.account.signedOut.createAccount"),
            },
          ]}
        />

        <form
          className="mt-5 space-y-4"
          onSubmit={submit}
          noValidate
          aria-busy={busy === "password" || undefined}
        >
          {creating && (
            <Field
              id={`${ids}-name`}
              label={t("settings.account.signedOut.name")}
              type="text"
              autoComplete="name"
              maxLength={64}
              value={name}
              onChange={(event) => setName(event.target.value)}
              readOnly={locked}
            />
          )}
          <Field
            id={`${ids}-email`}
            label={t("settings.account.signedOut.email")}
            type="email"
            inputMode="email"
            autoComplete="email"
            spellCheck={false}
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            readOnly={locked}
          />
          <Field
            id={`${ids}-password`}
            label={t("settings.account.signedOut.password")}
            hint={
              creating
                ? t("settings.account.signedOut.passwordHint")
                : undefined
            }
            type="password"
            autoComplete={creating ? "new-password" : "current-password"}
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            readOnly={locked}
          />

          {error && (
            <p role="alert" className="text-sm text-error break-words">
              {t(error)}
            </p>
          )}

          <Button
            type="submit"
            className="w-full"
            disabled={googlePending || !email.trim() || !password}
          >
            {busy === "password"
              ? t(
                  creating
                    ? "settings.account.signedOut.creating"
                    : "settings.account.signedOut.signingIn",
                )
              : t(
                  creating
                    ? "settings.account.signedOut.createAccount"
                    : "settings.account.signedOut.signIn",
                )}
          </Button>
        </form>

        {googleAvailable && (
          <>
            <div className="my-5 flex items-center gap-3 text-xs text-muted">
              <span className="h-px flex-1 bg-hairline" aria-hidden="true" />
              {t("settings.account.signedOut.or")}
              <span className="h-px flex-1 bg-hairline" aria-hidden="true" />
            </div>
            {waiting ? (
              <div
                role="status"
                className="flex items-center gap-3 rounded-lg border border-hairline bg-surface-strong/50 py-1.5 ps-3 pe-1.5"
              >
                <Loader2
                  className="h-4 w-4 shrink-0 animate-spin text-muted motion-reduce:animate-none"
                  aria-hidden="true"
                />
                <span className="min-w-0 flex-1 text-sm text-body">
                  {t("settings.account.signedOut.googleWaiting")}
                </span>
                <Button
                  id={googleId}
                  variant="ghost"
                  size="sm"
                  onClick={() => void commands.accountCancelGoogle()}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            ) : (
              <Button
                id={googleId}
                variant="secondary"
                className="w-full"
                disabled={locked}
                onClick={() =>
                  void run("google", () => commands.accountSignInGoogle())
                }
              >
                <GoogleLogo size={15} />
                {t("settings.account.signedOut.google")}
              </Button>
            )}
          </>
        )}

        <p className="mt-6 text-center text-xs leading-relaxed text-muted text-pretty">
          {t("settings.account.signedOut.privacy")}
        </p>
      </div>
    </section>
  );
};
